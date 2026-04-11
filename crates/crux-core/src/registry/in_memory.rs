/// Adapter: in-memory registry backend for tests and single-process agents.
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::types::id::TaskId;

use super::backend::RegistryBackend;
use super::error::RegistryErr;

#[derive(Debug, Clone, Default)]
pub struct InMemoryBackend {
    store: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl RegistryBackend for InMemoryBackend {
    async fn get(&self, id: &TaskId) -> Result<Option<Vec<u8>>, RegistryErr> {
        let store = self.store.read().map_err(|e| RegistryErr::Storage(e.to_string()))?;
        Ok(store.get(id.as_str()).cloned())
    }

    async fn put(&self, id: &TaskId, data: Vec<u8>) -> Result<(), RegistryErr> {
        let mut store = self.store.write().map_err(|e| RegistryErr::Storage(e.to_string()))?;
        store.insert(id.as_str().to_string(), data);
        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<TaskId>, RegistryErr> {
        let store = self.store.read().map_err(|e| RegistryErr::Storage(e.to_string()))?;
        let ids = store
            .keys()
            .filter(|k| k.starts_with(prefix))
            .filter_map(|k| k.parse().ok())
            .collect();
        Ok(ids)
    }

    async fn cas(
        &self,
        id: &TaskId,
        expected: Vec<u8>,
        new: Vec<u8>,
    ) -> Result<bool, RegistryErr> {
        let mut store = self.store.write().map_err(|e| RegistryErr::Storage(e.to_string()))?;
        let key = id.as_str().to_string();
        match store.get(&key) {
            Some(current) if *current == expected => {
                store.insert(key, new);
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_and_get() {
        let backend = InMemoryBackend::new();
        let id = TaskId::new();
        backend.put(&id, b"hello".to_vec()).await.unwrap();

        let data = backend.get(&id).await.unwrap();
        assert_eq!(data, Some(b"hello".to_vec()));
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let backend = InMemoryBackend::new();
        let id = TaskId::new();
        assert_eq!(backend.get(&id).await.unwrap(), None);
    }

    #[tokio::test]
    async fn cas_succeeds_on_match() {
        let backend = InMemoryBackend::new();
        let id = TaskId::new();
        backend.put(&id, b"v1".to_vec()).await.unwrap();

        let ok = backend.cas(&id, b"v1".to_vec(), b"v2".to_vec()).await.unwrap();
        assert!(ok);
        assert_eq!(backend.get(&id).await.unwrap(), Some(b"v2".to_vec()));
    }

    #[tokio::test]
    async fn cas_fails_on_mismatch() {
        let backend = InMemoryBackend::new();
        let id = TaskId::new();
        backend.put(&id, b"v1".to_vec()).await.unwrap();

        let ok = backend.cas(&id, b"wrong".to_vec(), b"v2".to_vec()).await.unwrap();
        assert!(!ok);
        assert_eq!(backend.get(&id).await.unwrap(), Some(b"v1".to_vec()));
    }
}
