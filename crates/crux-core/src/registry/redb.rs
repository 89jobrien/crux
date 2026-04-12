/// Adapter: redb registry backend — pure-Rust embedded key-value store.
///
/// Single table: tasks(&str => &[u8]).
/// Wraps a redb::Database in Arc for Send + Sync.
use std::sync::Arc;

use ::redb::{Database, ReadOnlyTable, ReadableDatabase, ReadableTable, TableDefinition};

use crate::types::id::TaskId;

use super::backend::RegistryBackend;
use super::error::RegistryErr;

const TASKS: TableDefinition<&str, &[u8]> = TableDefinition::new("tasks");

fn storage_err(e: impl std::fmt::Display) -> RegistryErr {
    RegistryErr::Storage(e.to_string())
}

#[derive(Clone)]
pub struct RedbBackend {
    db: Arc<Database>,
}

impl RedbBackend {
    /// Open (or create) a redb database at the given path.
    pub fn open(path: &str) -> Result<Self, RegistryErr> {
        let db = Database::create(path).map_err(storage_err)?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Open a redb database backed by a temporary file. Useful for tests.
    pub fn temporary() -> Result<Self, RegistryErr> {
        let tmpfile = tempfile::NamedTempFile::new().map_err(storage_err)?;
        let db = Database::create(tmpfile.path()).map_err(storage_err)?;
        // Leak the NamedTempFile so the file persists for the database lifetime.
        // The OS will reclaim it on process exit.
        std::mem::forget(tmpfile);
        Ok(Self { db: Arc::new(db) })
    }
}

impl RegistryBackend for RedbBackend {
    async fn get(&self, id: &TaskId) -> Result<Option<Vec<u8>>, RegistryErr> {
        let read_txn = self.db.begin_read().map_err(storage_err)?;

        let table: ReadOnlyTable<&str, &[u8]> = match read_txn.open_table(TASKS) {
            Ok(t) => t,
            Err(::redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(storage_err(e)),
        };

        match table.get(id.as_str()).map_err(storage_err)? {
            Some(guard) => Ok(Some(guard.value().to_vec())),
            None => Ok(None),
        }
    }

    async fn put(&self, id: &TaskId, data: Vec<u8>) -> Result<(), RegistryErr> {
        let write_txn = self.db.begin_write().map_err(storage_err)?;
        {
            let mut table = write_txn.open_table(TASKS).map_err(storage_err)?;
            table
                .insert(id.as_str(), data.as_slice())
                .map_err(storage_err)?;
        }
        write_txn.commit().map_err(storage_err)?;
        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<TaskId>, RegistryErr> {
        let read_txn = self.db.begin_read().map_err(storage_err)?;

        let table: ReadOnlyTable<&str, &[u8]> = match read_txn.open_table(TASKS) {
            Ok(t) => t,
            Err(::redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(storage_err(e)),
        };

        let mut ids = Vec::new();
        for entry in table.iter().map_err(storage_err)? {
            let (key, _val) = entry.map_err(storage_err)?;
            let s: &str = key.value();
            if prefix.is_empty() || s.starts_with(prefix) {
                ids.push(s.parse::<TaskId>().unwrap_or_else(|_| unreachable!()));
            }
        }

        Ok(ids)
    }

    async fn cas(&self, id: &TaskId, expected: Vec<u8>, new: Vec<u8>) -> Result<bool, RegistryErr> {
        let write_txn = self.db.begin_write().map_err(storage_err)?;
        let swapped = {
            let mut table = write_txn.open_table(TASKS).map_err(storage_err)?;

            let current = table
                .get(id.as_str())
                .map_err(storage_err)?
                .map(|g| g.value().to_vec());

            match current {
                Some(ref cur) if *cur == expected => {
                    table
                        .insert(id.as_str(), new.as_slice())
                        .map_err(storage_err)?;
                    true
                }
                _ => false,
            }
        };

        if swapped {
            write_txn.commit().map_err(storage_err)?;
        }
        // If not swapped, the write_txn is dropped without commit (implicit abort).
        Ok(swapped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend() -> RedbBackend {
        RedbBackend::temporary().expect("temp db")
    }

    #[tokio::test]
    async fn put_and_get() {
        let b = backend();
        let id = TaskId::new();
        b.put(&id, b"hello".to_vec()).await.unwrap();
        assert_eq!(b.get(&id).await.unwrap(), Some(b"hello".to_vec()));
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let b = backend();
        let id = TaskId::new();
        assert_eq!(b.get(&id).await.unwrap(), None);
    }

    #[tokio::test]
    async fn put_overwrites_existing() {
        let b = backend();
        let id = TaskId::new();
        b.put(&id, b"v1".to_vec()).await.unwrap();
        b.put(&id, b"v2".to_vec()).await.unwrap();
        assert_eq!(b.get(&id).await.unwrap(), Some(b"v2".to_vec()));
    }

    #[tokio::test]
    async fn list_with_prefix() {
        let b = backend();
        let id1 = TaskId::new();
        let id2 = TaskId::new();
        b.put(&id1, b"a".to_vec()).await.unwrap();
        b.put(&id2, b"b".to_vec()).await.unwrap();

        let all = b.list("").await.unwrap();
        assert_eq!(all.len(), 2);

        let none = b.list("ZZZZ_no_match").await.unwrap();
        assert!(none.is_empty());
    }

    #[tokio::test]
    async fn cas_succeeds_on_match() {
        let b = backend();
        let id = TaskId::new();
        b.put(&id, b"v1".to_vec()).await.unwrap();

        let ok = b.cas(&id, b"v1".to_vec(), b"v2".to_vec()).await.unwrap();
        assert!(ok);
        assert_eq!(b.get(&id).await.unwrap(), Some(b"v2".to_vec()));
    }

    #[tokio::test]
    async fn cas_fails_on_mismatch() {
        let b = backend();
        let id = TaskId::new();
        b.put(&id, b"v1".to_vec()).await.unwrap();

        let ok = b.cas(&id, b"wrong".to_vec(), b"v2".to_vec()).await.unwrap();
        assert!(!ok);
        assert_eq!(b.get(&id).await.unwrap(), Some(b"v1".to_vec()));
    }

    #[tokio::test]
    async fn cas_fails_on_missing() {
        let b = backend();
        let id = TaskId::new();

        let ok = b.cas(&id, b"v1".to_vec(), b"v2".to_vec()).await.unwrap();
        assert!(!ok);
    }
}
