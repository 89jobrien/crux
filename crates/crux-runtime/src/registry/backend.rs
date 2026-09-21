//! Byte-storage port for persistent task registry adapters.

/// Port: storage backend for the task registry.
///
/// Adapters implement this trait (in-memory, sqlite, postgres).
/// The TaskRegistry uses it through dynamic dispatch.
use crate::types::id::TaskId;

use super::error::RegistryErr;

pub trait RegistryBackend: Send + Sync {
    /// Reads the serialized task for an ID, if present.
    fn get(
        &self,
        id: &TaskId,
    ) -> impl std::future::Future<Output = Result<Option<Vec<u8>>, RegistryErr>> + Send;

    /// Stores serialized task data under an ID.
    fn put(
        &self,
        id: &TaskId,
        data: Vec<u8>,
    ) -> impl std::future::Future<Output = Result<(), RegistryErr>> + Send;

    /// Lists task IDs whose string representation begins with the prefix.
    fn list(
        &self,
        prefix: &str,
    ) -> impl std::future::Future<Output = Result<Vec<TaskId>, RegistryErr>> + Send;

    /// Atomically replaces task data when the current bytes equal `expected`.
    fn cas(
        &self,
        id: &TaskId,
        expected: Vec<u8>,
        new: Vec<u8>,
    ) -> impl std::future::Future<Output = Result<bool, RegistryErr>> + Send;
}
