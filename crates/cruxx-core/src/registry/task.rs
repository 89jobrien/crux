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

    /// Update a task's status using a bounded CAS retry loop.
    ///
    /// Attempts up to 3 read-modify-CAS cycles. If all attempts fail due to
    /// concurrent writes, returns `RegistryErr::Conflict`.
    pub async fn update_status(&self, id: &TaskId, status: TaskStatus) -> Result<(), RegistryErr> {
        const MAX_ATTEMPTS: u32 = 3;
        for attempt in 0..MAX_ATTEMPTS {
            let old_data = self
                .backend
                .get(id)
                .await?
                .ok_or_else(|| RegistryErr::NotFound(id.to_string()))?;
            let mut task: Task = serde_json::from_slice(&old_data)?;
            task.status = status.clone();
            let new_data = serde_json::to_vec(&task)?;
            let swapped = self.backend.cas(id, old_data, new_data).await?;
            if swapped {
                return Ok(());
            }
            // CAS lost — another writer modified the task; retry unless exhausted.
            if attempt == MAX_ATTEMPTS - 1 {
                return Err(RegistryErr::Conflict(id.to_string()));
            }
        }
        Err(RegistryErr::Conflict(id.to_string()))
    }

    /// Save an execution checkpoint (Crux snapshot) into the task using a bounded CAS retry loop.
    ///
    /// Attempts up to 3 read-modify-CAS cycles. If all attempts fail due to
    /// concurrent writes, returns `RegistryErr::Conflict`.
    pub async fn checkpoint<T: Serialize>(
        &self,
        id: &TaskId,
        cruxx: &Crux<T>,
    ) -> Result<(), RegistryErr> {
        let snapshot = cruxx.to_snapshot().map_err(RegistryErr::Serialization)?;
        const MAX_ATTEMPTS: u32 = 3;
        for attempt in 0..MAX_ATTEMPTS {
            let old_data = self
                .backend
                .get(id)
                .await?
                .ok_or_else(|| RegistryErr::NotFound(id.to_string()))?;
            let mut task: Task = serde_json::from_slice(&old_data)?;
            task.checkpoint = Some(snapshot.clone());
            task.attempts += 1;
            let new_data = serde_json::to_vec(&task)?;
            let swapped = self.backend.cas(id, old_data, new_data).await?;
            if swapped {
                return Ok(());
            }
            // CAS lost — another writer modified the task; retry unless exhausted.
            if attempt == MAX_ATTEMPTS - 1 {
                return Err(RegistryErr::Conflict(id.to_string()));
            }
        }
        Err(RegistryErr::Conflict(id.to_string()))
    }

    /// List all pending tasks of the given kind. Pass "" to return all pending tasks
    /// regardless of kind. Kind-based filtering is done in memory after listing all
    /// tasks, because task keys are ULIDs (not prefixed by kind).
    pub async fn pending(&self, kind: &str) -> Result<Vec<Task>, RegistryErr> {
        let ids = self.backend.list("").await?;
        let mut tasks = Vec::new();
        for id in &ids {
            if let Some(data) = self.backend.get(id).await? {
                let task: Task = serde_json::from_slice(&data)?;
                if task.status == TaskStatus::Pending && (kind.is_empty() || task.kind == kind) {
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
    use crate::registry::backend::RegistryBackend;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A backend wrapper that fails CAS for the first `fail_count` attempts,
    /// then delegates to the inner backend.
    struct FailingCasBackend {
        inner: InMemoryBackend,
        cas_fail_remaining: Arc<AtomicU32>,
    }

    impl FailingCasBackend {
        fn new(fail_count: u32) -> Self {
            Self {
                inner: InMemoryBackend::new(),
                cas_fail_remaining: Arc::new(AtomicU32::new(fail_count)),
            }
        }
    }

    impl RegistryBackend for FailingCasBackend {
        async fn get(&self, id: &TaskId) -> Result<Option<Vec<u8>>, RegistryErr> {
            self.inner.get(id).await
        }

        async fn put(&self, id: &TaskId, data: Vec<u8>) -> Result<(), RegistryErr> {
            self.inner.put(id, data).await
        }

        async fn list(&self, prefix: &str) -> Result<Vec<TaskId>, RegistryErr> {
            self.inner.list(prefix).await
        }

        async fn cas(
            &self,
            id: &TaskId,
            expected: Vec<u8>,
            new: Vec<u8>,
        ) -> Result<bool, RegistryErr> {
            let remaining = self.cas_fail_remaining.load(Ordering::SeqCst);
            if remaining > 0 {
                self.cas_fail_remaining.fetch_sub(1, Ordering::SeqCst);
                // Return false (CAS lost) without touching the store.
                return Ok(false);
            }
            self.inner.cas(id, expected, new).await
        }
    }
    use cruxx_types::testing::{crux_ok, step_ok};

    fn make_registry() -> TaskRegistry<InMemoryBackend> {
        TaskRegistry::new(InMemoryBackend::new())
    }

    fn make_cruxx() -> cruxx_types::crux_value::Crux<String> {
        crux_ok("test", "result".into(), vec![step_ok("fetch", 42, Some(serde_json::json!("data")))])
    }

    #[tokio::test]
    async fn submit_and_get() {
        let reg = make_registry();
        let id = reg
            .submit("build", serde_json::json!({"repo": "cruxx"}))
            .await
            .unwrap();
        let task = reg.get(&id).await.unwrap();
        assert_eq!(task.kind, "build");
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.input["repo"], "cruxx");
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
    async fn checkpoint_saves_cruxx_snapshot() {
        let reg = make_registry();
        let id = reg
            .submit("analyze", serde_json::json!("input"))
            .await
            .unwrap();
        let cruxx = make_cruxx();
        reg.checkpoint(&id, &cruxx).await.unwrap();

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
        reg.checkpoint(&id, &make_cruxx()).await.unwrap();

        let cp = reg.load_checkpoint(&id).await.unwrap().unwrap();
        assert_eq!(cp.steps[0].name, "fetch");
    }

    #[tokio::test]
    async fn pending_filters_by_status() {
        let reg = make_registry();
        let id1 = reg.submit("build", serde_json::json!(1)).await.unwrap();
        let _id2 = reg.submit("build", serde_json::json!(2)).await.unwrap();
        reg.update_status(&id1, TaskStatus::Done).await.unwrap();

        // List all pending (kind = "") — only the second build task remains.
        let pending = reg.pending("").await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].input, serde_json::json!(2));
    }

    #[tokio::test]
    async fn pending_filters_by_kind() {
        let reg = make_registry();
        let _build1 = reg.submit("build", serde_json::json!(1)).await.unwrap();
        let _build2 = reg.submit("build", serde_json::json!(2)).await.unwrap();
        let _deploy = reg.submit("deploy", serde_json::json!(3)).await.unwrap();

        // pending("build") must return only the two build tasks.
        let build_pending = reg.pending("build").await.unwrap();
        assert_eq!(build_pending.len(), 2);
        assert!(build_pending.iter().all(|t| t.kind == "build"));

        // pending("deploy") must return only the deploy task.
        let deploy_pending = reg.pending("deploy").await.unwrap();
        assert_eq!(deploy_pending.len(), 1);
        assert_eq!(deploy_pending[0].kind, "deploy");

        // After completing one build, pending("build") drops to one.
        reg.update_status(&_build1, TaskStatus::Done).await.unwrap();
        let build_pending2 = reg.pending("build").await.unwrap();
        assert_eq!(build_pending2.len(), 1);
    }

    #[tokio::test]
    async fn multiple_checkpoints_increment_attempts() {
        let reg = make_registry();
        let id = reg.submit("retry", serde_json::json!(null)).await.unwrap();
        let cruxx = make_cruxx();
        reg.checkpoint(&id, &cruxx).await.unwrap();
        reg.checkpoint(&id, &cruxx).await.unwrap();
        let task = reg.get(&id).await.unwrap();
        assert_eq!(task.attempts, 2);
    }

    // -- CAS retry behavior ---------------------------------------------------

    #[tokio::test]
    async fn update_status_succeeds_after_cas_retries() {
        // Two CAS failures then success on the third attempt.
        let backend = FailingCasBackend::new(2);
        let reg = TaskRegistry::new(backend);
        let id = reg.submit("build", serde_json::json!(null)).await.unwrap();
        reg.update_status(&id, TaskStatus::Running).await.unwrap();
        let task = reg.get(&id).await.unwrap();
        assert_eq!(task.status, TaskStatus::Running);
    }

    #[tokio::test]
    async fn update_status_returns_conflict_when_all_retries_fail() {
        // Three CAS failures exhaust all attempts.
        let backend = FailingCasBackend::new(3);
        let reg = TaskRegistry::new(backend);
        let id = reg.submit("build", serde_json::json!(null)).await.unwrap();
        let err = reg
            .update_status(&id, TaskStatus::Running)
            .await
            .unwrap_err();
        assert!(matches!(err, RegistryErr::Conflict(_)));
    }

    #[tokio::test]
    async fn checkpoint_succeeds_after_cas_retries() {
        // Two CAS failures then success on the third attempt.
        let backend = FailingCasBackend::new(2);
        let reg = TaskRegistry::new(backend);
        let id = reg
            .submit("analyze", serde_json::json!(null))
            .await
            .unwrap();
        let cruxx = make_cruxx();
        reg.checkpoint(&id, &cruxx).await.unwrap();
        let task = reg.get(&id).await.unwrap();
        assert!(task.checkpoint.is_some());
        assert_eq!(task.attempts, 1);
    }

    #[tokio::test]
    async fn checkpoint_returns_conflict_when_all_retries_fail() {
        // Three CAS failures exhaust all attempts.
        let backend = FailingCasBackend::new(3);
        let reg = TaskRegistry::new(backend);
        let id = reg
            .submit("analyze", serde_json::json!(null))
            .await
            .unwrap();
        let cruxx = make_cruxx();
        let err = reg.checkpoint(&id, &cruxx).await.unwrap_err();
        assert!(matches!(err, RegistryErr::Conflict(_)));
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

#[cfg(test)]
mod proptest_task_status {
    use super::*;
    use crate::registry::InMemoryBackend;
    use proptest::prelude::*;

    fn arb_status() -> impl Strategy<Value = TaskStatus> {
        prop_oneof![
            Just(TaskStatus::Pending),
            Just(TaskStatus::Running),
            Just(TaskStatus::Done),
            Just(TaskStatus::Failed),
        ]
    }

    fn arb_kind() -> impl Strategy<Value = String> {
        "[a-z]{3,10}"
    }

    proptest! {
        /// Any status transition applied to a submitted task should persist correctly.
        #[test]
        fn update_status_persists_any_status(
            kind in arb_kind(),
            target_status in arb_status(),
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let reg = TaskRegistry::new(InMemoryBackend::new());
                let id = reg.submit(&kind, serde_json::json!(null)).await.unwrap();
                reg.update_status(&id, target_status.clone()).await.unwrap();
                let task = reg.get(&id).await.unwrap();
                prop_assert_eq!(task.status, target_status);
                Ok(())
            })?;
        }

        /// After any transition, get() returns the correct kind.
        #[test]
        fn update_status_does_not_change_kind(
            kind in arb_kind(),
            target_status in arb_status(),
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let reg = TaskRegistry::new(InMemoryBackend::new());
                let id = reg.submit(&kind, serde_json::json!(null)).await.unwrap();
                reg.update_status(&id, target_status).await.unwrap();
                let task = reg.get(&id).await.unwrap();
                prop_assert_eq!(&task.kind, &kind);
                Ok(())
            })?;
        }

        /// Applying the same status twice is idempotent.
        #[test]
        fn update_status_idempotent(
            kind in arb_kind(),
            status in arb_status(),
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let reg = TaskRegistry::new(InMemoryBackend::new());
                let id = reg.submit(&kind, serde_json::json!(null)).await.unwrap();
                reg.update_status(&id, status.clone()).await.unwrap();
                reg.update_status(&id, status.clone()).await.unwrap();
                let task = reg.get(&id).await.unwrap();
                prop_assert_eq!(task.status, status);
                Ok(())
            })?;
        }

        /// Pending tasks appear in pending() list; non-pending ones do not.
        #[test]
        fn pending_list_reflects_status(
            kind in arb_kind(),
            non_pending in prop_oneof![
                Just(TaskStatus::Running),
                Just(TaskStatus::Done),
                Just(TaskStatus::Failed),
            ],
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let reg = TaskRegistry::new(InMemoryBackend::new());
                let id1 = reg.submit(&kind, serde_json::json!(1)).await.unwrap();
                let id2 = reg.submit(&kind, serde_json::json!(2)).await.unwrap();
                // Move id2 to a non-pending status.
                reg.update_status(&id2, non_pending).await.unwrap();

                let pending = reg.pending(&kind).await.unwrap();
                prop_assert_eq!(pending.len(), 1);
                prop_assert_eq!(&pending[0].id, &id1);
                Ok(())
            })?;
        }
    }
}
