//! TaskManager -- high-level project task management.

use crux_runtime::registry::RegistryBackend;

use crate::error::TaskErr;
use crate::types::*;
use crux_types::id::TaskId;

/// High-level project task manager, generic over storage backend.
pub struct TaskManager<B: RegistryBackend> {
    backend: B,
}

impl<B: RegistryBackend> TaskManager<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    pub async fn add(&self, _spec: TaskSpec) -> Result<TaskId, TaskErr> {
        let _ = &self.backend;
        todo!()
    }

    pub async fn get(&self, _id: &TaskId) -> Result<ProjectTask, TaskErr> {
        todo!()
    }

    pub async fn update(&self, _id: &TaskId, _patch: TaskPatch) -> Result<(), TaskErr> {
        todo!()
    }

    pub async fn list(&self, _filter: TaskFilter) -> Result<Vec<ProjectTask>, TaskErr> {
        todo!()
    }

    pub async fn ready(&self) -> Result<Vec<ProjectTask>, TaskErr> {
        todo!()
    }

    pub async fn blocked(&self) -> Result<Vec<ProjectTask>, TaskErr> {
        todo!()
    }

    pub async fn by_priority(&self) -> Result<Vec<ProjectTask>, TaskErr> {
        todo!()
    }

    pub async fn block(&self, _id: &TaskId, _blocker: &TaskId) -> Result<(), TaskErr> {
        todo!()
    }

    pub async fn unblock(&self, _id: &TaskId, _blocker: &TaskId) -> Result<(), TaskErr> {
        todo!()
    }

    pub async fn stats(&self) -> Result<TaskStats, TaskErr> {
        todo!()
    }
}
