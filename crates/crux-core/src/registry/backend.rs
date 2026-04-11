/// Port: storage backend for the task registry.
///
/// Adapters implement this trait (in-memory, sqlite, postgres).
/// The TaskRegistry uses it through dynamic dispatch.
use crate::types::id::TaskId;

use super::error::RegistryErr;

pub trait RegistryBackend: Send + Sync {
    fn get(
        &self,
        id: &TaskId,
    ) -> impl std::future::Future<Output = Result<Option<Vec<u8>>, RegistryErr>> + Send;

    fn put(
        &self,
        id: &TaskId,
        data: Vec<u8>,
    ) -> impl std::future::Future<Output = Result<(), RegistryErr>> + Send;

    fn list(
        &self,
        prefix: &str,
    ) -> impl std::future::Future<Output = Result<Vec<TaskId>, RegistryErr>> + Send;

    fn cas(
        &self,
        id: &TaskId,
        expected: Vec<u8>,
        new: Vec<u8>,
    ) -> impl std::future::Future<Output = Result<bool, RegistryErr>> + Send;
}
