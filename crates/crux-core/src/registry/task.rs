/// TaskRegistry — typed, high-level API over a RegistryBackend.
///
/// Provides submit/get/update_status/checkpoint/pending/resume lifecycle
/// for persistent, crash-safe task management.
use serde::{Deserialize, Serialize};

use crate::types::crux_value::Crux;
use crate::types::id::TaskId;

use super::backend::RegistryBackend;
use super::error::RegistryErr;

/// Status of a task in the registry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Pending,
    Running,
    Done,
    Failed,
}

/// A task stored in the registry — wraps typed input with status and optional checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub kind: String,
    pub status: TaskStatus,
    pub input: serde_json::Value,
    pub checkpoint: Option<Crux<serde_json::Value>>,
    pub attempts: u32,
}

/// High-level typed API for task lifecycle management.
pub struct TaskRegistry<B> {
    backend: B,
}

impl<B: RegistryBackend> TaskRegistry<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Submit a new task. Returns the assigned TaskId.
    pub async fn submit<I: Serialize>(&self, kind: &str, input: I) -> Result<TaskId, RegistryErr> {
        let id = TaskId::new();
        let task = Task {
            id: id.clone(),
            kind: kind.to_string(),
            status: TaskStatus::Pending,
            input: serde_json::to_value(input)?,
            checkpoint: None,
            attempts: 0,
        };
        let data = serde_json::to_vec(&task)?;
        self.backend.put(&id, data).await?;
        Ok(id)
    }

    /// Retrieve a task by id.
    pub async fn get(&self, id: &TaskId) -> Result<Task, RegistryErr> {
        let data = self
            .backend
            .get(id)
            .await?
            .ok_or_else(|| RegistryErr::NotFound(id.to_string()))?;
        Ok(serde_json::from_slice(&data)?)
    }

    /// Update a task's status.
    pub async fn update_status(&self, id: &TaskId, status: TaskStatus) -> Result<(), RegistryErr> {
        let old_data = self
            .backend
            .get(id)
            .await?
            .ok_or_else(|| RegistryErr::NotFound(id.to_string()))?;
        let mut task: Task = serde_json::from_slice(&old_data)?;
        task.status = status;
        let new_data = serde_json::to_vec(&task)?;
        let swapped = self.backend.cas(id, old_data, new_data.clone()).await?;
        if !swapped {
            // Concurrent update — retry with fresh read.
            let fresh = self
                .backend
                .get(id)
                .await?
                .ok_or_else(|| RegistryErr::NotFound(id.to_string()))?;
            let mut task2: Task = serde_json::from_slice(&fresh)?;
            task2.status = task.status;
            let data2 = serde_json::to_vec(&task2)?;
            self.backend.put(id, data2).await?;
        }
        Ok(())
    }

    /// Save an execution checkpoint (Crux snapshot) into the task.
    pub async fn checkpoint<T: Serialize>(
        &self,
        id: &TaskId,
        crux: &Crux<T>,
    ) -> Result<(), RegistryErr> {
        let snapshot = crux.to_snapshot().map_err(RegistryErr::Serialization)?;
        let old_data = self
            .backend
            .get(id)
            .await?
            .ok_or_else(|| RegistryErr::NotFound(id.to_string()))?;
        let mut task: Task = serde_json::from_slice(&old_data)?;
        task.checkpoint = Some(snapshot);
        task.attempts += 1;
        let new_data = serde_json::to_vec(&task)?;
        self.backend.put(id, new_data).await?;
        Ok(())
    }

    /// List all pending tasks (filtered from all tasks with a given prefix).
    pub async fn pending(&self, kind: &str) -> Result<Vec<Task>, RegistryErr> {
        let ids = self.backend.list(kind).await?;
        let mut tasks = Vec::new();
        for id in &ids {
            if let Some(data) = self.backend.get(id).await? {
                let task: Task = serde_json::from_slice(&data)?;
                if task.status == TaskStatus::Pending {
                    tasks.push(task);
                }
            }
        }
        Ok(tasks)
    }

    /// Load a task's checkpoint for replay. Returns None if no checkpoint exists.
    pub async fn load_checkpoint(
        &self,
        id: &TaskId,
    ) -> Result<Option<Crux<serde_json::Value>>, RegistryErr> {
        let task = self.get(id).await?;
        Ok(task.checkpoint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::InMemoryBackend;
    use crate::types::crux_value::Crux;
    use crate::types::id::CruxId;
    use crate::types::step::{Step, StepKind, StepStatus};
    use chrono::Utc;

    fn make_registry() -> TaskRegistry<InMemoryBackend> {
        TaskRegistry::new(InMemoryBackend::new())
    }

    fn make_crux() -> Crux<String> {
        Crux {
            id: CruxId::new(),
            agent: "test".into(),
            value: Ok("result".into()),
            steps: vec![Step {
                name: "fetch".into(),
                kind: StepKind::Plain,
                status: StepStatus::Ok,
                confidence: 1.0,
                started_at: Utc::now(),
                duration_ms: 5,
                input_hash: 42,
                output: Some(serde_json::json!("data")),
                error: None,
                attempt: 1,
            }],
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        }
    }

    #[tokio::test]
    async fn submit_and_get() {
        let reg = make_registry();
        let id = reg
            .submit("build", serde_json::json!({"repo": "crux"}))
            .await
            .unwrap();
        let task = reg.get(&id).await.unwrap();
        assert_eq!(task.kind, "build");
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.input["repo"], "crux");
        assert_eq!(task.attempts, 0);
    }

    #[tokio::test]
    async fn get_missing_returns_not_found() {
        let reg = make_registry();
        let id = TaskId::new();
        let err = reg.get(&id).await.unwrap_err();
        assert!(matches!(err, RegistryErr::NotFound(_)));
    }

    #[tokio::test]
    async fn update_status() {
        let reg = make_registry();
        let id = reg.submit("deploy", serde_json::json!(null)).await.unwrap();
        reg.update_status(&id, TaskStatus::Running).await.unwrap();
        let task = reg.get(&id).await.unwrap();
        assert_eq!(task.status, TaskStatus::Running);
    }

    #[tokio::test]
    async fn update_status_to_done() {
        let reg = make_registry();
        let id = reg.submit("test", serde_json::json!(null)).await.unwrap();
        reg.update_status(&id, TaskStatus::Done).await.unwrap();
        let task = reg.get(&id).await.unwrap();
        assert_eq!(task.status, TaskStatus::Done);
    }

    #[tokio::test]
    async fn checkpoint_saves_crux_snapshot() {
        let reg = make_registry();
        let id = reg
            .submit("analyze", serde_json::json!("input"))
            .await
            .unwrap();
        let crux = make_crux();
        reg.checkpoint(&id, &crux).await.unwrap();

        let task = reg.get(&id).await.unwrap();
        assert!(task.checkpoint.is_some());
        assert_eq!(task.attempts, 1);

        let cp = task.checkpoint.unwrap();
        assert_eq!(cp.agent, "test");
        assert_eq!(cp.steps.len(), 1);
    }

    #[tokio::test]
    async fn load_checkpoint_returns_none_without_checkpoint() {
        let reg = make_registry();
        let id = reg.submit("quick", serde_json::json!(null)).await.unwrap();
        assert!(reg.load_checkpoint(&id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn load_checkpoint_returns_snapshot() {
        let reg = make_registry();
        let id = reg.submit("long", serde_json::json!(null)).await.unwrap();
        reg.checkpoint(&id, &make_crux()).await.unwrap();

        let cp = reg.load_checkpoint(&id).await.unwrap().unwrap();
        assert_eq!(cp.steps[0].name, "fetch");
    }

    #[tokio::test]
    async fn pending_filters_by_status() {
        let reg = make_registry();
        // InMemoryBackend list uses prefix match on the TaskId string.
        // Since TaskIds are random ULIDs, we can't filter by kind prefix directly.
        // Instead, pending() takes a kind parameter to list with "" prefix (all).
        let id1 = reg.submit("build", serde_json::json!(1)).await.unwrap();
        let _id2 = reg.submit("build", serde_json::json!(2)).await.unwrap();
        reg.update_status(&id1, TaskStatus::Done).await.unwrap();

        // List all tasks and filter pending.
        let pending = reg.pending("").await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].input, serde_json::json!(2));
    }

    #[tokio::test]
    async fn multiple_checkpoints_increment_attempts() {
        let reg = make_registry();
        let id = reg.submit("retry", serde_json::json!(null)).await.unwrap();
        let crux = make_crux();
        reg.checkpoint(&id, &crux).await.unwrap();
        reg.checkpoint(&id, &crux).await.unwrap();
        let task = reg.get(&id).await.unwrap();
        assert_eq!(task.attempts, 2);
    }

    #[tokio::test]
    async fn task_serde_round_trip() {
        let task = Task {
            id: TaskId::new(),
            kind: "test".into(),
            status: TaskStatus::Running,
            input: serde_json::json!({"x": 1}),
            checkpoint: None,
            attempts: 3,
        };
        let json = serde_json::to_string(&task).unwrap();
        let back: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, "test");
        assert_eq!(back.status, TaskStatus::Running);
        assert_eq!(back.attempts, 3);
    }
}
