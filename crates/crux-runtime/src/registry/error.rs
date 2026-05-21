/// Registry-specific errors.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegistryErr {
    #[error("task not found: {0}")]
    NotFound(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("CAS conflict on task {0}")]
    Conflict(String),
}
