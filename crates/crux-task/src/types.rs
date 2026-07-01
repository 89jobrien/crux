//! Domain types for project task management.

use std::collections::HashMap;

use crux_types::id::TaskId;
use crux_types::task::{DependencyKind, Priority, TaskLabel};
use serde::{Deserialize, Serialize};

/// A dependency edge: this task is related to `target` by `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub target: TaskId,
    pub kind: DependencyKind,
}

/// Lifecycle status of a project task.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTaskStatus {
    #[default]
    Open,
    InProgress,
    Done,
    Blocked,
    Cancelled,
}

/// Specification for creating a task. Pure data -- no ID or timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub title: String,
    pub description: Option<String>,
    pub priority: Priority,
    pub status: ProjectTaskStatus,
    pub labels: Vec<TaskLabel>,
    pub dependencies: Vec<Dependency>,
}

/// A stored project task -- TaskSpec + identity + timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTask {
    pub id: TaskId,
    pub spec: TaskSpec,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Filter for querying tasks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskFilter {
    pub status: Option<ProjectTaskStatus>,
    pub priority: Option<Priority>,
    pub label: Option<TaskLabel>,
}

/// Patch for updating a task. All fields optional -- only set fields are applied.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskPatch {
    pub status: Option<ProjectTaskStatus>,
    pub priority: Option<Priority>,
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub add_labels: Vec<TaskLabel>,
    pub remove_labels: Vec<TaskLabel>,
    pub add_dependencies: Vec<Dependency>,
    pub remove_dependencies: Vec<TaskId>,
}

/// Aggregate statistics across all tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStats {
    pub total: usize,
    pub by_status: HashMap<ProjectTaskStatus, usize>,
    pub by_priority: HashMap<Priority, usize>,
}
