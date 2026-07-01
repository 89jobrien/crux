//! Adapter: SQLite-backed RegistryBackend for crux-task.

use std::sync::Mutex;

use crux_runtime::registry::RegistryBackend;
use crux_runtime::registry::error::RegistryErr;
use crux_types::id::TaskId;
use rusqlite::Connection;
use rusqlite::OptionalExtension;

use crate::error::TaskErr;

pub struct SqliteBackend {
    conn: Mutex<Connection>,
}

impl SqliteBackend {
    pub fn open(path: &str) -> Result<Self, TaskErr> {
        let conn =
            Connection::open(path).map_err(|e| TaskErr::Storage(format!("sqlite open: {e}")))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                data BLOB NOT NULL
            )",
        )
        .map_err(|e| TaskErr::Storage(format!("sqlite init: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
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
        let result = stmt
            .query_row([id.as_str()], |row| row.get(0))
            .optional()
            .map_err(|e| RegistryErr::Storage(e.to_string()))?;
        Ok(result)
    }

    async fn put(&self, id: &TaskId, data: Vec<u8>) -> Result<(), RegistryErr> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| RegistryErr::Storage(e.to_string()))?;
        conn.execute(
            "INSERT OR REPLACE INTO tasks (id, data) VALUES (?1, ?2)",
            rusqlite::params![id.as_str(), data],
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
            .prepare("SELECT id FROM tasks WHERE id LIKE ?1")
            .map_err(|e| RegistryErr::Storage(e.to_string()))?;
        let pattern = format!("{prefix}%");
        let ids = stmt
            .query_map([&pattern], |row| {
                let s: String = row.get(0)?;
                Ok(s.parse::<TaskId>().unwrap())
            })
            .map_err(|e| RegistryErr::Storage(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    }

    async fn cas(&self, id: &TaskId, expected: Vec<u8>, new: Vec<u8>) -> Result<bool, RegistryErr> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| RegistryErr::Storage(e.to_string()))?;
        let current: Option<Vec<u8>> = conn
            .prepare("SELECT data FROM tasks WHERE id = ?1")
            .map_err(|e| RegistryErr::Storage(e.to_string()))?
            .query_row([id.as_str()], |row| row.get(0))
            .optional()
            .map_err(|e| RegistryErr::Storage(e.to_string()))?;
        match current {
            Some(ref c) if *c == expected => {
                conn.execute(
                    "UPDATE tasks SET data = ?1 WHERE id = ?2",
                    rusqlite::params![new, id.as_str()],
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

    #[tokio::test]
    async fn manager_over_sqlite() {
        use crate::{ProjectTaskStatus, TaskManager, TaskSpec};
        use crux_types::task::Priority;

        let backend = SqliteBackend::open(":memory:").unwrap();
        let mgr = TaskManager::new(backend);
        let spec = TaskSpec {
            title: "Test task".into(),
            description: None,
            priority: Priority::P1,
            status: ProjectTaskStatus::Open,
            labels: vec![],
            dependencies: vec![],
        };
        let id = mgr.add(spec).await.unwrap();
        let task = mgr.get(&id).await.unwrap();
        assert_eq!(task.spec.title, "Test task");
    }
}
