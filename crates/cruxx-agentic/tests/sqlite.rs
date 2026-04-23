use cruxx_agentic::sqlite;
use cruxx_script::HandlerRegistry;
use serde_json::json;
use tempfile::NamedTempFile;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    sqlite::register(&mut r);
    r
}

/// Create a temp SQLite DB with a `tasks` table and return its path string.
fn setup_db() -> NamedTempFile {
    let f = NamedTempFile::new().unwrap();
    let conn = rusqlite::Connection::open(f.path()).unwrap();
    conn.execute_batch(
        "CREATE TABLE tasks (
            id   INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            done INTEGER NOT NULL DEFAULT 0
        );",
    )
    .unwrap();
    f
}

#[tokio::test]
async fn exec_creates_table() {
    let db = NamedTempFile::new().unwrap();
    let reg = registry();
    let handler = reg.get_handler("sqlite::exec").unwrap();
    let result = handler(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "CREATE TABLE foo (id INTEGER PRIMARY KEY)"
        }
    }))
    .await
    .unwrap();
    assert!(result["rows_affected"].is_number());
}

#[tokio::test]
async fn exec_missing_db_returns_error() {
    let reg = registry();
    let handler = reg.get_handler("sqlite::exec").unwrap();
    let result = handler(json!({"args": {"sql": "SELECT 1"}})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn exec_missing_sql_returns_error() {
    let db = NamedTempFile::new().unwrap();
    let reg = registry();
    let handler = reg.get_handler("sqlite::exec").unwrap();
    let result = handler(json!({"args": {"db": db.path().to_str().unwrap()}})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn insert_returns_rowid() {
    let db = setup_db();
    let reg = registry();
    let handler = reg.get_handler("sqlite::insert").unwrap();
    let result = handler(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "INSERT INTO tasks (name) VALUES (:name)",
            "params": { ":name": "write tests" }
        }
    }))
    .await
    .unwrap();
    assert_eq!(result["last_insert_rowid"], 1);
}

#[tokio::test]
async fn query_many_returns_inserted_rows() {
    let db = setup_db();
    let reg = registry();
    let insert = reg.get_handler("sqlite::insert").unwrap();
    insert(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "INSERT INTO tasks (name) VALUES (:name)",
            "params": { ":name": "alpha" }
        }
    }))
    .await
    .unwrap();
    insert(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "INSERT INTO tasks (name) VALUES (:name)",
            "params": { ":name": "beta" }
        }
    }))
    .await
    .unwrap();

    let query = reg.get_handler("sqlite::query_many").unwrap();
    let result = query(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "SELECT id, name, done FROM tasks ORDER BY id"
        }
    }))
    .await
    .unwrap();
    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], "alpha");
    assert_eq!(rows[1]["name"], "beta");
}

#[tokio::test]
async fn query_one_returns_single_row() {
    let db = setup_db();
    let reg = registry();
    let insert = reg.get_handler("sqlite::insert").unwrap();
    insert(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "INSERT INTO tasks (name) VALUES (:name)",
            "params": { ":name": "solo" }
        }
    }))
    .await
    .unwrap();

    let query = reg.get_handler("sqlite::query_one").unwrap();
    let result = query(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "SELECT id, name FROM tasks WHERE id = 1"
        }
    }))
    .await
    .unwrap();
    assert_eq!(result["row"]["name"], "solo");
}

#[tokio::test]
async fn query_one_errors_on_no_rows() {
    let db = setup_db();
    let reg = registry();
    let query = reg.get_handler("sqlite::query_one").unwrap();
    let result = query(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "SELECT * FROM tasks"
        }
    }))
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn query_one_errors_on_multiple_rows() {
    let db = setup_db();
    let reg = registry();
    let insert = reg.get_handler("sqlite::insert").unwrap();
    for name in &["a", "b"] {
        insert(json!({
            "args": {
                "db": db.path().to_str().unwrap(),
                "sql": "INSERT INTO tasks (name) VALUES (:name)",
                "params": { ":name": name }
            }
        }))
        .await
        .unwrap();
    }
    let query = reg.get_handler("sqlite::query_one").unwrap();
    let result = query(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "SELECT * FROM tasks"
        }
    }))
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn update_modifies_row() {
    let db = setup_db();
    let reg = registry();
    let insert = reg.get_handler("sqlite::insert").unwrap();
    insert(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "INSERT INTO tasks (name) VALUES (:name)",
            "params": { ":name": "old" }
        }
    }))
    .await
    .unwrap();

    let update = reg.get_handler("sqlite::update").unwrap();
    let result = update(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "UPDATE tasks SET name = :name WHERE id = 1",
            "params": { ":name": "new" }
        }
    }))
    .await
    .unwrap();
    assert_eq!(result["rows_affected"], 1);

    let query = reg.get_handler("sqlite::query_one").unwrap();
    let check = query(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "SELECT name FROM tasks WHERE id = 1"
        }
    }))
    .await
    .unwrap();
    assert_eq!(check["row"]["name"], "new");
}

#[tokio::test]
async fn delete_removes_row() {
    let db = setup_db();
    let reg = registry();
    let insert = reg.get_handler("sqlite::insert").unwrap();
    insert(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "INSERT INTO tasks (name) VALUES (:name)",
            "params": { ":name": "to-delete" }
        }
    }))
    .await
    .unwrap();

    let delete = reg.get_handler("sqlite::delete").unwrap();
    let result = delete(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "DELETE FROM tasks WHERE id = :id",
            "params": { ":id": 1 }
        }
    }))
    .await
    .unwrap();
    assert_eq!(result["rows_affected"], 1);

    let query = reg.get_handler("sqlite::query_many").unwrap();
    let check = query(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "SELECT * FROM tasks"
        }
    }))
    .await
    .unwrap();
    assert_eq!(check["rows"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn upsert_inserts_then_replaces() {
    let db = NamedTempFile::new().unwrap();
    {
        let conn = rusqlite::Connection::open(db.path()).unwrap();
        conn.execute_batch("CREATE TABLE kv (key TEXT PRIMARY KEY, val TEXT);")
            .unwrap();
    }
    let reg = registry();
    let upsert = reg.get_handler("sqlite::upsert").unwrap();

    upsert(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "INSERT OR REPLACE INTO kv (key, val) VALUES (:key, :val)",
            "params": { ":key": "x", ":val": "1" }
        }
    }))
    .await
    .unwrap();

    upsert(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "INSERT OR REPLACE INTO kv (key, val) VALUES (:key, :val)",
            "params": { ":key": "x", ":val": "2" }
        }
    }))
    .await
    .unwrap();

    let query = reg.get_handler("sqlite::query_one").unwrap();
    let result = query(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "SELECT val FROM kv WHERE key = 'x'"
        }
    }))
    .await
    .unwrap();
    assert_eq!(result["row"]["val"], "2");
}

#[tokio::test]
async fn query_many_empty_table_returns_empty_array() {
    let db = setup_db();
    let reg = registry();
    let query = reg.get_handler("sqlite::query_many").unwrap();
    let result = query(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "SELECT * FROM tasks"
        }
    }))
    .await
    .unwrap();
    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 0);
}

#[tokio::test]
async fn params_bind_correctly() {
    let db = setup_db();
    let reg = registry();
    let insert = reg.get_handler("sqlite::insert").unwrap();
    insert(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "INSERT INTO tasks (name, done) VALUES (:name, :done)",
            "params": { ":name": "paramtest", ":done": 1 }
        }
    }))
    .await
    .unwrap();

    let query = reg.get_handler("sqlite::query_one").unwrap();
    let result = query(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "SELECT done FROM tasks WHERE name = :name",
            "params": { ":name": "paramtest" }
        }
    }))
    .await
    .unwrap();
    assert_eq!(result["row"]["done"], 1);
}

#[tokio::test]
async fn all_handlers_registered() {
    let reg = registry();
    for name in &[
        "sqlite::exec",
        "sqlite::query_one",
        "sqlite::query_many",
        "sqlite::insert",
        "sqlite::update",
        "sqlite::delete",
        "sqlite::upsert",
    ] {
        assert!(
            reg.get_handler(name).is_some(),
            "handler not registered: {name}"
        );
    }
}

/// Conformance: every constant in handlers.rs must resolve to a registered handler.
#[test]
fn constants_match_registered_handlers() {
    use cruxx_agentic::handlers::{
        SQLITE_DELETE, SQLITE_EXEC, SQLITE_INSERT, SQLITE_QUERY_MANY, SQLITE_QUERY_ONE,
        SQLITE_UPDATE, SQLITE_UPSERT,
    };
    let reg = registry();
    for name in &[
        SQLITE_EXEC,
        SQLITE_QUERY_ONE,
        SQLITE_QUERY_MANY,
        SQLITE_INSERT,
        SQLITE_UPDATE,
        SQLITE_DELETE,
        SQLITE_UPSERT,
    ] {
        assert!(
            reg.get_handler(name).is_some(),
            "constant '{name}' has no matching registered handler"
        );
    }
}

/// Conformance: register_all includes sqlite handlers.
#[test]
fn register_all_includes_sqlite() {
    use cruxx_agentic::handlers::SQLITE_EXEC;
    let mut reg = HandlerRegistry::new();
    cruxx_agentic::register_all(&mut reg);
    assert!(
        reg.get_handler(SQLITE_EXEC).is_some(),
        "register_all must include sqlite::exec"
    );
}
