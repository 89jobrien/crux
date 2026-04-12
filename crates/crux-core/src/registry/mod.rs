/// Task registry — persistent, crash-safe task management.
pub mod backend;
pub mod error;
pub mod in_memory;
pub mod task;

#[cfg(feature = "redb")]
pub mod redb;

pub use backend::RegistryBackend;
pub use error::RegistryErr;
pub use in_memory::InMemoryBackend;
pub use task::{Task, TaskRegistry, TaskStatus};

#[cfg(feature = "redb")]
pub use self::redb::RedbBackend;
