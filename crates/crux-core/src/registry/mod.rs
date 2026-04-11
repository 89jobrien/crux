/// Task registry — persistent, crash-safe task management.
pub mod backend;
pub mod error;
pub mod in_memory;

pub use backend::RegistryBackend;
pub use error::RegistryErr;
pub use in_memory::InMemoryBackend;
