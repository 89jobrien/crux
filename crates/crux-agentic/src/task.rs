//! Pipeline step handlers for crux-task.
//!
//! All handlers require a `"db"` arg pointing to a redb file.

use crux_runtime::prelude::CruxErr;
use crux_runtime::registry::RedbBackend;
use crux_script::HandlerRegistry;
use crux_task::{ProjectTaskStatus, TaskFilter, TaskManager, TaskPatch, TaskSpec};
use crux_types::id::TaskId;
use crux_types::task::{Priority, TaskLabel};
use serde_json::{Value, json};

use crate::error::{AgenticError, opt_str, require_str};

fn open_manager(input: &Value) -> Result<TaskManager<RedbBackend>, CruxErr> {
    let db_path = require_str(input, "db")?;
    let backend =
        RedbBackend::open(db_path).map_err(|e| CruxErr::step_failed("task", e.to_string()))?;
    Ok(TaskManager::new(backend))
}

fn parse_priority(s: &str) -> Result<Priority, AgenticError> {
    match s {
        "p0" => Ok(Priority::P0),
        "p1" => Ok(Priority::P1),
        "p2" => Ok(Priority::P2),
        "p3" => Ok(Priority::P3),
        other => Err(AgenticError::Other(format!("invalid priority: {other}"))),
    }
}

fn parse_status(s: &str) -> Result<ProjectTaskStatus, AgenticError> {
    match s {
        "open" => Ok(ProjectTaskStatus::Open),
        "in_progress" => Ok(ProjectTaskStatus::InProgress),
        "done" => Ok(ProjectTaskStatus::Done),
        "blocked" => Ok(ProjectTaskStatus::Blocked),
        "cancelled" => Ok(ProjectTaskStatus::Cancelled),
        other => Err(AgenticError::Other(format!("invalid status: {other}"))),
    }
}

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value("task::create", |input: Value| async move {
        let mgr = open_manager(&input)?;
        let title = require_str(&input, "title")?;
        let priority = opt_str(&input, "priority").unwrap_or("p2");
        let priority = parse_priority(priority).map_err(CruxErr::from)?;
        let status = opt_str(&input, "status").unwrap_or("open");
        let status = parse_status(status).map_err(CruxErr::from)?;
        let labels: Vec<TaskLabel> = input
            .get("args")
            .and_then(|a| a.get("labels"))
            .and_then(|l| l.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| TaskLabel(s.into())))
                    .collect()
            })
            .unwrap_or_default();
        let description = opt_str(&input, "description").map(String::from);

        let spec = TaskSpec {
            title: title.to_string(),
            description,
            priority,
            status,
            labels,
            dependencies: vec![],
        };
        let id = mgr
            .add(spec)
            .await
            .map_err(|e| CruxErr::step_failed("task::create", e.to_string()))?;
        Ok(json!({ "id": id.as_str() }))
    });

    registry.handler_value("task::update", |input: Value| async move {
        let mgr = open_manager(&input)?;
        let id_str = require_str(&input, "id")?;
        let id: TaskId = id_str
            .parse()
            .map_err(|e| CruxErr::step_failed("task::update", format!("bad id: {e}")))?;
        let status = opt_str(&input, "status")
            .map(parse_status)
            .transpose()
            .map_err(CruxErr::from)?;
        let priority = opt_str(&input, "priority")
            .map(parse_priority)
            .transpose()
            .map_err(CruxErr::from)?;
        let patch = TaskPatch {
            status,
            priority,
            ..Default::default()
        };
        mgr.update(&id, patch)
            .await
            .map_err(|e| CruxErr::step_failed("task::update", e.to_string()))?;
        Ok(json!({ "updated": id_str }))
    });

    registry.handler_value("task::list", |input: Value| async move {
        let mgr = open_manager(&input)?;
        let status = opt_str(&input, "status")
            .map(parse_status)
            .transpose()
            .map_err(CruxErr::from)?;
        let priority = opt_str(&input, "priority")
            .map(parse_priority)
            .transpose()
            .map_err(CruxErr::from)?;
        let label = opt_str(&input, "label").map(|s| TaskLabel(s.into()));
        let filter = TaskFilter {
            status,
            priority,
            label,
        };
        let tasks = mgr
            .list(filter)
            .await
            .map_err(|e| CruxErr::step_failed("task::list", e.to_string()))?;
        Ok(serde_json::to_value(&tasks).unwrap())
    });

    registry.handler_value("task::ready", |input: Value| async move {
        let mgr = open_manager(&input)?;
        let tasks = mgr
            .ready()
            .await
            .map_err(|e| CruxErr::step_failed("task::ready", e.to_string()))?;
        Ok(serde_json::to_value(&tasks).unwrap())
    });
}
