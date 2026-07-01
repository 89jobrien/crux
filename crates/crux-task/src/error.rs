//! Error types for crux-task.

/// Errors that can occur in task management operations.
#[derive(Debug, thiserror::Error)]
pub enum TaskErr {
    #[error("task not found: {0}")]
    NotFound(String),
    #[error("cycle detected: adding dependency would create a cycle")]
    CycleDetected,
    #[error("storage error: {0}")]
    Storage(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
