/// Task registry — persistent, crash-safe task management.
pub mod backend;
pub mod error;
pub mod in_memory;
pub mod task;

#[cfg(feature = "sqlite")]
pub mod sqlite;

pub use backend::RegistryBackend;
pub use error::RegistryErr;
pub use in_memory::InMemoryBackend;
pub use task::{Task, TaskRegistry, TaskStatus};

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteBackend;
