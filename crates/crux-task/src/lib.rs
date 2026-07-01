//! crux-task -- project task management for crux.
//!
//! Rich, dependency-aware task tracking usable by both crux agents
//! at runtime and developers from the CLI.

pub mod error;
pub mod manager;
pub mod types;

#[cfg(feature = "sqlite")]
pub mod sqlite;

pub use error::TaskErr;
pub use manager::TaskManager;
pub use types::{
    Dependency, ProjectTask, ProjectTaskStatus, TaskFilter, TaskPatch, TaskSpec, TaskStats,
};
