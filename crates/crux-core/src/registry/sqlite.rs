/// Adapter: SQLite registry backend using rusqlite.
///
/// Single table: tasks(id TEXT PRIMARY KEY, data BLOB NOT NULL).
/// Wraps a rusqlite::Connection in Arc<Mutex<>> for Send + Sync.
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, params};

use crate::types::id::TaskId;

use super::backend::RegistryBackend;
use super::error::RegistryErr;

#[derive(Clone)]
pub struct SqliteBackend {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteBackend {
    /// Open (or create) a SQLite database at the given path.
    pub fn open(path: &str) -> Result<Self, RegistryErr> {
        let conn = Connection::open(path).map_err(|e| RegistryErr::Storage(e.to_string()))?;
        Self::init(conn)
    }

    /// Open an in-memory SQLite database. Useful for tests.
    pub fn in_memory() -> Result<Self, RegistryErr> {
        let conn = Connection::open_in_memory().map_err(|e| RegistryErr::Storage(e.to_string()))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self, RegistryErr> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tasks (
                id   TEXT PRIMARY KEY NOT NULL,
                data BLOB NOT NULL
            );",
        )
        .map_err(|e| RegistryErr::Storage(e.to_string()))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

impl RegistryBackend for SqliteBackend {
    async fn get(&self, id: &TaskId) -> Result<Option<Vec<u8>>, RegistryErr> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| RegistryErr::Storage(e.to_string()))?;

        let mut stmt = conn
            .prepare("SELECT data FROM tasks WHERE id = ?1")
            .map_err(|e| RegistryErr::Storage(e.to_string()))?;

        let mut rows = stmt
            .query(params![id.as_str()])
            .map_err(|e| RegistryErr::Storage(e.to_string()))?;

        match rows
            .next()
            .map_err(|e| RegistryErr::Storage(e.to_string()))?
        {
            Some(row) => {
                let data: Vec<u8> = row
                    .get(0)
                    .map_err(|e| RegistryErr::Storage(e.to_string()))?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    async fn put(&self, id: &TaskId, data: Vec<u8>) -> Result<(), RegistryErr> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| RegistryErr::Storage(e.to_string()))?;

        conn.execute(
            "INSERT INTO tasks (id, data) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            params![id.as_str(), data],
        )
        .map_err(|e| RegistryErr::Storage(e.to_string()))?;

        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<TaskId>, RegistryErr> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| RegistryErr::Storage(e.to_string()))?;

        let mut stmt = conn
            .prepare("SELECT id FROM tasks WHERE id LIKE ?1 ESCAPE '\\'")
            .map_err(|e| RegistryErr::Storage(e.to_string()))?;

        // Escape LIKE special chars in prefix, then append % for prefix match.
        let pattern = format!(
            "{}%",
            prefix
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );

        let ids: Result<Vec<TaskId>, _> = stmt
            .query_map(params![pattern], |row| {
                let s: String = row.get(0)?;
                Ok(s)
            })
            .map_err(|e| RegistryErr::Storage(e.to_string()))?
            .map(|res| {
                res.map_err(|e| RegistryErr::Storage(e.to_string()))
                    .map(|s| s.parse::<TaskId>().unwrap_or_else(|_| unreachable!()))
            })
            .collect();

        ids
    }

    async fn cas(&self, id: &TaskId, expected: Vec<u8>, new: Vec<u8>) -> Result<bool, RegistryErr> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| RegistryErr::Storage(e.to_string()))?;

        // Read current value and conditionally update within the same lock.
        let current: Option<Vec<u8>> = {
            let mut stmt = conn
                .prepare("SELECT data FROM tasks WHERE id = ?1")
                .map_err(|e| RegistryErr::Storage(e.to_string()))?;

            let mut rows = stmt
                .query(params![id.as_str()])
                .map_err(|e| RegistryErr::Storage(e.to_string()))?;

            match rows
                .next()
                .map_err(|e| RegistryErr::Storage(e.to_string()))?
            {
                Some(row) => {
                    let data: Vec<u8> = row
                        .get(0)
                        .map_err(|e| RegistryErr::Storage(e.to_string()))?;
                    Some(data)
                }
                None => None,
            }
        };

        match current {
            Some(ref cur) if *cur == expected => {
                conn.execute(
                    "UPDATE tasks SET data = ?1 WHERE id = ?2",
                    params![new, id.as_str()],
                )
                .map_err(|e| RegistryErr::Storage(e.to_string()))?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend() -> SqliteBackend {
        SqliteBackend::in_memory().expect("in-memory db")
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

        // List with empty prefix returns all.
        let all = b.list("").await.unwrap();
        assert_eq!(all.len(), 2);

        // List with a prefix that matches nothing.
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
