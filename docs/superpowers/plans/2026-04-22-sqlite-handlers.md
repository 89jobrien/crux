# sqlite Handlers for crux-agentic — Implementation Plan

status: done

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `sqlite` handler module to `crux-agentic` exposing seven CRUD operations as
named pipeline step handlers, tested with unit tests, property tests, conformance tests, and
fuzz targets.

**Architecture:** Each handler is a thin async closure registered under `sqlite::<name>`. All
share the arg shape `{ db, sql, params }`. A `rusqlite` connection is opened per call (no
connection pool — preflight use case is low-frequency). Tests use `tempfile::NamedTempFile` for
isolated SQLite databases.

**Tech Stack:** Rust 2024, `rusqlite` 0.32 (bundled), `proptest` 1.11, `tempfile` 3,
`tokio::test`, `cargo-fuzz` / `libfuzzer-sys`.

---

## File Map

| Path                                       | Action             | Purpose                                    |
| ------------------------------------------ | ------------------ | ------------------------------------------ |
| `crates/crux-agentic/Cargo.toml`           | Modify             | Add `rusqlite` dep + `sqlite` feature      |
| `crates/crux-agentic/src/sqlite.rs`        | Create             | All 7 handler implementations              |
| `crates/crux-agentic/src/handlers.rs`      | Modify             | Add `sqlite::*` name constants             |
| `crates/crux-agentic/src/lib.rs`           | Modify             | Register sqlite handlers in `register_all` |
| `crates/crux-agentic/tests/sqlite.rs`      | Create             | Unit + integration + conformance tests     |
| `crates/crux-agentic/tests/sqlite_prop.rs` | Create             | Property tests (proptest)                  |
| `fuzz/fuzz_targets/sqlite_args.rs`         | Create             | Fuzz target for handler arg parsing        |
| `fuzz/Cargo.toml`                          | Modify (or create) | Add sqlite fuzz target                     |

---

## Task 1: Add `rusqlite` dependency

**Files:**

- Modify: `crates/crux-agentic/Cargo.toml`

- [ ] **Step 1: Add the dependency**

Add to `[dependencies]` in `crates/crux-agentic/Cargo.toml`:

```toml
rusqlite = { version = "0.32", features = ["bundled"] }
```

The `bundled` feature compiles sqlite3 from source — no system lib required, no dynamic linking.

- [ ] **Step 2: Verify it compiles**

```bash
cd /Users/joe/dev/crux
cargo check -p crux-agentic
```

Expected: no errors. Warnings about unused deps are fine at this stage.

- [ ] **Step 3: Commit**

```bash
git -C /Users/joe/dev/crux add crates/crux-agentic/Cargo.toml Cargo.lock
git -C /Users/joe/dev/crux commit -m "chore(agentic): add rusqlite bundled dep"
```

---

## Task 2: Add handler name constants

**Files:**

- Modify: `crates/crux-agentic/src/handlers.rs`

- [ ] **Step 1: Write the failing test (constants exist)**

Add to `crates/crux-agentic/tests/handler_constants.rs` (file already exists — append):

```rust
#[test]
fn sqlite_constants_defined() {
    use crux_agentic::handlers::{
        SQLITE_DELETE, SQLITE_EXEC, SQLITE_INSERT, SQLITE_QUERY_MANY, SQLITE_QUERY_ONE,
        SQLITE_UPDATE, SQLITE_UPSERT,
    };
    assert_eq!(SQLITE_EXEC, "sqlite::exec");
    assert_eq!(SQLITE_QUERY_ONE, "sqlite::query_one");
    assert_eq!(SQLITE_QUERY_MANY, "sqlite::query_many");
    assert_eq!(SQLITE_INSERT, "sqlite::insert");
    assert_eq!(SQLITE_UPDATE, "sqlite::update");
    assert_eq!(SQLITE_DELETE, "sqlite::delete");
    assert_eq!(SQLITE_UPSERT, "sqlite::upsert");
}
```

- [ ] **Step 2: Run to verify it fails**

```bash
cd /Users/joe/dev/crux
cargo nextest run -p crux-agentic sqlite_constants_defined
```

Expected: compile error — constants not defined.

- [ ] **Step 3: Add the constants**

Append to `crates/crux-agentic/src/handlers.rs`:

```rust
// sqlite
pub const SQLITE_EXEC: &str = "sqlite::exec";
pub const SQLITE_QUERY_ONE: &str = "sqlite::query_one";
pub const SQLITE_QUERY_MANY: &str = "sqlite::query_many";
pub const SQLITE_INSERT: &str = "sqlite::insert";
pub const SQLITE_UPDATE: &str = "sqlite::update";
pub const SQLITE_DELETE: &str = "sqlite::delete";
pub const SQLITE_UPSERT: &str = "sqlite::upsert";
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo nextest run -p crux-agentic sqlite_constants_defined
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git -C /Users/joe/dev/crux add crates/crux-agentic/src/handlers.rs \
  crates/crux-agentic/tests/handler_constants.rs
git -C /Users/joe/dev/crux commit -m "feat(agentic): add sqlite handler name constants"
```

---

## Task 3: Implement `sqlite.rs` — core helpers

**Files:**

- Create: `crates/crux-agentic/src/sqlite.rs`

- [ ] **Step 1: Write failing tests for helpers**

Create `crates/crux-agentic/tests/sqlite.rs`:

```rust
use crux_agentic::sqlite;
use crux_script::HandlerRegistry;
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
```

- [ ] **Step 2: Run to verify it fails**

```bash
cd /Users/joe/dev/crux
cargo nextest run -p crux-agentic exec_creates_table
```

Expected: compile error — `sqlite` module not found.

- [ ] **Step 3: Implement `sqlite.rs` skeleton + `exec`**

Create `crates/crux-agentic/src/sqlite.rs`:

```rust
//! SQLite step handlers for crux-script pipelines.
//!
//! All handlers share the arg shape:
//! `{ "db": "<path>", "sql": "<query>", "params": { ":name": "value" } }`

use crux_runtime::prelude::CruxErr;
use crux_script::HandlerRegistry;
use rusqlite::{Connection, named_params, types::Value as SqlValue};
use serde_json::{Map, Value, json};

use crate::error::{AgenticError, require_str};

// ── internal helpers ────────────────────────────────────────────────────────

fn open(input: &Value) -> Result<(Connection, String), AgenticError> {
    let db_path = require_str(input, "db")?;
    let conn = Connection::open(db_path)
        .map_err(|e| AgenticError::Other(format!("sqlite open {db_path}: {e}")))?;
    Ok((conn, db_path.to_string()))
}

fn require_sql(input: &Value) -> Result<String, AgenticError> {
    require_str(input, "sql").map(|s| s.to_string())
}

/// Convert a JSON params object `{ ":name": value }` to rusqlite param pairs.
/// Only string, number, bool, and null values are supported.
fn json_params(input: &Value) -> Vec<(String, SqlValue)> {
    let Some(obj) = input
        .get("args")
        .and_then(|a| a.get("params"))
        .and_then(|p| p.as_object())
    else {
        return vec![];
    };
    obj.iter()
        .map(|(k, v)| {
            let sql_val = match v {
                Value::Null => SqlValue::Null,
                Value::Bool(b) => SqlValue::Integer(*b as i64),
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        SqlValue::Integer(i)
                    } else {
                        SqlValue::Real(n.as_f64().unwrap_or(0.0))
                    }
                }
                Value::String(s) => SqlValue::Text(s.clone()),
                _ => SqlValue::Null,
            };
            (k.clone(), sql_val)
        })
        .collect()
}

/// Execute a query and return all rows as a JSON array.
fn rows_to_json(
    conn: &Connection,
    sql: &str,
    params: &[(String, SqlValue)],
) -> Result<Vec<Value>, AgenticError> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| AgenticError::Other(format!("prepare: {e}")))?;

    let col_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

    let param_refs: Vec<(&str, &dyn rusqlite::ToSql)> = params
        .iter()
        .map(|(k, v)| (k.as_str(), v as &dyn rusqlite::ToSql))
        .collect();

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            let mut map = Map::new();
            for (i, name) in col_names.iter().enumerate() {
                let val: rusqlite::types::Value = row.get(i)?;
                let json_val = match val {
                    SqlValue::Null => Value::Null,
                    SqlValue::Integer(n) => json!(n),
                    SqlValue::Real(f) => json!(f),
                    SqlValue::Text(s) => Value::String(s),
                    SqlValue::Blob(b) => Value::String(base64_encode(&b)),
                };
                map.insert(name.clone(), json_val);
            }
            Ok(map)
        })
        .map_err(|e| AgenticError::Other(format!("query: {e}")))?;

    let mut result = vec![];
    for row in rows {
        result.push(Value::Object(
            row.map_err(|e| AgenticError::Other(format!("row: {e}")))?,
        ));
    }
    Ok(result)
}

fn base64_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        write!(s, "{b:02x}").ok();
    }
    s
}

fn to_crux(e: AgenticError) -> CruxErr {
    CruxErr::from(e)
}

// ── handler registration ─────────────────────────────────────────────────────

pub fn register(registry: &mut HandlerRegistry) {
    // sqlite::exec — DDL / fire-and-forget DML
    registry.handler_value("sqlite::exec", |input: Value| async move {
        let (conn, _) = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let param_refs: Vec<(&str, &dyn rusqlite::ToSql)> = params
            .iter()
            .map(|(k, v)| (k.as_str(), v as &dyn rusqlite::ToSql))
            .collect();
        let rows_affected = conn
            .execute(sql.as_str(), param_refs.as_slice())
            .map_err(|e| CruxErr::step_failed("sqlite::exec", e.to_string()))?;
        Ok(json!({ "rows_affected": rows_affected }))
    });

    // sqlite::query_many — SELECT returning array
    registry.handler_value("sqlite::query_many", |input: Value| async move {
        let (conn, _) = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let rows = rows_to_json(&conn, &sql, &params).map_err(to_crux)?;
        Ok(json!({ "rows": rows }))
    });

    // sqlite::query_one — SELECT expecting exactly one row
    registry.handler_value("sqlite::query_one", |input: Value| async move {
        let (conn, _) = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let mut rows = rows_to_json(&conn, &sql, &params).map_err(to_crux)?;
        match rows.len() {
            0 => Err(CruxErr::step_failed("sqlite::query_one", "no rows returned")),
            1 => Ok(json!({ "row": rows.remove(0) })),
            n => Err(CruxErr::step_failed(
                "sqlite::query_one",
                format!("expected 1 row, got {n}"),
            )),
        }
    });

    // sqlite::insert
    registry.handler_value("sqlite::insert", |input: Value| async move {
        let (conn, _) = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let param_refs: Vec<(&str, &dyn rusqlite::ToSql)> = params
            .iter()
            .map(|(k, v)| (k.as_str(), v as &dyn rusqlite::ToSql))
            .collect();
        conn.execute(sql.as_str(), param_refs.as_slice())
            .map_err(|e| CruxErr::step_failed("sqlite::insert", e.to_string()))?;
        let rowid = conn.last_insert_rowid();
        Ok(json!({ "last_insert_rowid": rowid }))
    });

    // sqlite::update
    registry.handler_value("sqlite::update", |input: Value| async move {
        let (conn, _) = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let param_refs: Vec<(&str, &dyn rusqlite::ToSql)> = params
            .iter()
            .map(|(k, v)| (k.as_str(), v as &dyn rusqlite::ToSql))
            .collect();
        let rows_affected = conn
            .execute(sql.as_str(), param_refs.as_slice())
            .map_err(|e| CruxErr::step_failed("sqlite::update", e.to_string()))?;
        Ok(json!({ "rows_affected": rows_affected }))
    });

    // sqlite::delete
    registry.handler_value("sqlite::delete", |input: Value| async move {
        let (conn, _) = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let param_refs: Vec<(&str, &dyn rusqlite::ToSql)> = params
            .iter()
            .map(|(k, v)| (k.as_str(), v as &dyn rusqlite::ToSql))
            .collect();
        let rows_affected = conn
            .execute(sql.as_str(), param_refs.as_slice())
            .map_err(|e| CruxErr::step_failed("sqlite::delete", e.to_string()))?;
        Ok(json!({ "rows_affected": rows_affected }))
    });

    // sqlite::upsert — INSERT OR REPLACE
    registry.handler_value("sqlite::upsert", |input: Value| async move {
        let (conn, _) = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let param_refs: Vec<(&str, &dyn rusqlite::ToSql)> = params
            .iter()
            .map(|(k, v)| (k.as_str(), v as &dyn rusqlite::ToSql))
            .collect();
        let rows_affected = conn
            .execute(sql.as_str(), param_refs.as_slice())
            .map_err(|e| CruxErr::step_failed("sqlite::upsert", e.to_string()))?;
        Ok(json!({ "rows_affected": rows_affected }))
    });
}
```

- [ ] **Step 4: Register the module in `lib.rs`**

In `crates/crux-agentic/src/lib.rs`, add:

```rust
pub mod sqlite;
```

And in `register_all_with_plugins`:

```rust
sqlite::register(registry);
```

(Add after `ctrl::register(registry);`)

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd /Users/joe/dev/crux
cargo nextest run -p crux-agentic exec_creates_table exec_missing_db_returns_error exec_missing_sql_returns_error
```

Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git -C /Users/joe/dev/crux add crates/crux-agentic/src/sqlite.rs \
  crates/crux-agentic/src/lib.rs \
  crates/crux-agentic/tests/sqlite.rs
git -C /Users/joe/dev/crux commit -m "feat(agentic): implement sqlite handler module with exec"
```

---

## Task 4: Full CRUD integration tests (red/green)

**Files:**

- Modify: `crates/crux-agentic/tests/sqlite.rs`

- [ ] **Step 1: Write all remaining failing tests**

Append to `crates/crux-agentic/tests/sqlite.rs`:

```rust
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
    // Create table with unique constraint for upsert
    {
        let conn = rusqlite::Connection::open(db.path()).unwrap();
        conn.execute_batch(
            "CREATE TABLE kv (key TEXT PRIMARY KEY, val TEXT);",
        )
        .unwrap();
    }
    let reg = registry();
    let upsert = reg.get_handler("sqlite::upsert").unwrap();

    // First upsert — insert
    upsert(json!({
        "args": {
            "db": db.path().to_str().unwrap(),
            "sql": "INSERT OR REPLACE INTO kv (key, val) VALUES (:key, :val)",
            "params": { ":key": "x", ":val": "1" }
        }
    }))
    .await
    .unwrap();

    // Second upsert — replace
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
```

- [ ] **Step 2: Run to verify all fail**

```bash
cd /Users/joe/dev/crux
cargo nextest run -p crux-agentic --test sqlite
```

Expected: multiple failures — impls missing or incomplete.

- [ ] **Step 3: Run after Task 3 implementation is in place**

```bash
cargo nextest run -p crux-agentic --test sqlite
```

Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git -C /Users/joe/dev/crux add crates/crux-agentic/tests/sqlite.rs
git -C /Users/joe/dev/crux commit -m "test(agentic): full CRUD integration tests for sqlite handlers"
```

---

## Task 5: Property tests

**Files:**

- Create: `crates/crux-agentic/tests/sqlite_prop.rs`

- [ ] **Step 1: Create property test file**

Create `crates/crux-agentic/tests/sqlite_prop.rs`:

```rust
//! Property-based tests for sqlite handlers using proptest.
//!
//! Properties under test:
//! 1. insert→query_one roundtrip: any valid name survives insert/read unchanged
//! 2. insert→update→query_one: updated value always reflects the new name
//! 3. insert→delete→query_many: table is always empty after delete of inserted row
//! 4. query_many count: N inserts → query_many returns exactly N rows

use crux_agentic::sqlite;
use crux_script::HandlerRegistry;
use proptest::prelude::*;
use serde_json::json;
use tempfile::NamedTempFile;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    sqlite::register(&mut r);
    r
}

fn setup_db() -> NamedTempFile {
    let f = NamedTempFile::new().unwrap();
    let conn = rusqlite::Connection::open(f.path()).unwrap();
    conn.execute_batch(
        "CREATE TABLE tasks (
            id   INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL
        );",
    )
    .unwrap();
    f
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn insert_query_one_roundtrip(name in "[a-zA-Z0-9 _-]{1,64}") {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let db = setup_db();
            let reg = registry();
            let insert = reg.get_handler("sqlite::insert").unwrap();
            insert(json!({
                "args": {
                    "db": db.path().to_str().unwrap(),
                    "sql": "INSERT INTO tasks (name) VALUES (:name)",
                    "params": { ":name": name }
                }
            }))
            .await
            .unwrap();

            let query = reg.get_handler("sqlite::query_one").unwrap();
            let result = query(json!({
                "args": {
                    "db": db.path().to_str().unwrap(),
                    "sql": "SELECT name FROM tasks WHERE id = 1"
                }
            }))
            .await
            .unwrap();
            prop_assert_eq!(result["row"]["name"].as_str().unwrap(), name.as_str());
            Ok(())
        }).unwrap();
    }

    #[test]
    fn insert_update_query_roundtrip(
        original in "[a-zA-Z]{1,32}",
        updated  in "[a-zA-Z]{1,32}",
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let db = setup_db();
            let reg = registry();

            reg.get_handler("sqlite::insert").unwrap()(json!({
                "args": {
                    "db": db.path().to_str().unwrap(),
                    "sql": "INSERT INTO tasks (name) VALUES (:name)",
                    "params": { ":name": original }
                }
            }))
            .await
            .unwrap();

            reg.get_handler("sqlite::update").unwrap()(json!({
                "args": {
                    "db": db.path().to_str().unwrap(),
                    "sql": "UPDATE tasks SET name = :name WHERE id = 1",
                    "params": { ":name": updated }
                }
            }))
            .await
            .unwrap();

            let result = reg.get_handler("sqlite::query_one").unwrap()(json!({
                "args": {
                    "db": db.path().to_str().unwrap(),
                    "sql": "SELECT name FROM tasks WHERE id = 1"
                }
            }))
            .await
            .unwrap();
            prop_assert_eq!(result["row"]["name"].as_str().unwrap(), updated.as_str());
            Ok(())
        }).unwrap();
    }

    #[test]
    fn n_inserts_gives_n_rows(n in 1usize..=20usize) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let db = setup_db();
            let reg = registry();
            let insert = reg.get_handler("sqlite::insert").unwrap();

            for i in 0..n {
                insert(json!({
                    "args": {
                        "db": db.path().to_str().unwrap(),
                        "sql": "INSERT INTO tasks (name) VALUES (:name)",
                        "params": { ":name": format!("task-{i}") }
                    }
                }))
                .await
                .unwrap();
            }

            let result = reg.get_handler("sqlite::query_many").unwrap()(json!({
                "args": {
                    "db": db.path().to_str().unwrap(),
                    "sql": "SELECT * FROM tasks"
                }
            }))
            .await
            .unwrap();
            let count = result["rows"].as_array().unwrap().len();
            prop_assert_eq!(count, n);
            Ok(())
        }).unwrap();
    }

    #[test]
    fn delete_leaves_empty_table(name in "[a-zA-Z]{1,32}") {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let db = setup_db();
            let reg = registry();

            reg.get_handler("sqlite::insert").unwrap()(json!({
                "args": {
                    "db": db.path().to_str().unwrap(),
                    "sql": "INSERT INTO tasks (name) VALUES (:name)",
                    "params": { ":name": name }
                }
            }))
            .await
            .unwrap();

            reg.get_handler("sqlite::delete").unwrap()(json!({
                "args": {
                    "db": db.path().to_str().unwrap(),
                    "sql": "DELETE FROM tasks WHERE id = 1"
                }
            }))
            .await
            .unwrap();

            let result = reg.get_handler("sqlite::query_many").unwrap()(json!({
                "args": {
                    "db": db.path().to_str().unwrap(),
                    "sql": "SELECT * FROM tasks"
                }
            }))
            .await
            .unwrap();
            let count = result["rows"].as_array().unwrap().len();
            prop_assert_eq!(count, 0);
            Ok(())
        }).unwrap();
    }
}
```

- [ ] **Step 2: Add `proptest` to dev-dependencies if not already present**

Check `crates/crux-agentic/Cargo.toml` `[dev-dependencies]`. If `proptest` is missing, add:

```toml
proptest = { workspace = true }
```

(It is already in `[workspace.dependencies]` in root `Cargo.toml`.)

- [ ] **Step 3: Run property tests**

```bash
cd /Users/joe/dev/crux
cargo nextest run -p crux-agentic --test sqlite_prop
```

Expected: all 4 property test cases PASS (64 cases each).

- [ ] **Step 4: Commit**

```bash
git -C /Users/joe/dev/crux add crates/crux-agentic/tests/sqlite_prop.rs \
  crates/crux-agentic/Cargo.toml
git -C /Users/joe/dev/crux commit -m "test(agentic): property tests for sqlite handler CRUD invariants"
```

---

## Task 6: Conformance test — handlers match registered constants

**Files:**

- Modify: `crates/crux-agentic/tests/sqlite.rs`

- [ ] **Step 1: Add conformance tests**

Append to `crates/crux-agentic/tests/sqlite.rs`:

```rust
/// Conformance: every constant in handlers.rs must resolve to a registered handler.
#[test]
fn constants_match_registered_handlers() {
    use crux_agentic::handlers::{
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
    use crux_agentic::handlers::SQLITE_EXEC;
    let mut reg = HandlerRegistry::new();
    crux_agentic::register_all(&mut reg);
    assert!(
        reg.get_handler(SQLITE_EXEC).is_some(),
        "register_all must include sqlite::exec"
    );
}
```

- [ ] **Step 2: Run conformance tests**

```bash
cd /Users/joe/dev/crux
cargo nextest run -p crux-agentic constants_match_registered_handlers register_all_includes_sqlite
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git -C /Users/joe/dev/crux add crates/crux-agentic/tests/sqlite.rs
git -C /Users/joe/dev/crux commit -m "test(agentic): conformance tests for sqlite constants and register_all"
```

---

## Task 7: Fuzz target for handler arg parsing

**Files:**

- Create or modify: `fuzz/Cargo.toml`
- Create: `fuzz/fuzz_targets/sqlite_args.rs`

- [ ] **Step 1: Check if fuzz crate exists**

```bash
ls /Users/joe/dev/crux/fuzz/ 2>/dev/null || echo "no fuzz dir"
```

If no fuzz dir exists, bootstrap it:

```bash
cd /Users/joe/dev/crux
cargo fuzz init
```

- [ ] **Step 2: Add sqlite fuzz target to `fuzz/Cargo.toml`**

Append to the `[[bin]]` sections in `fuzz/Cargo.toml`:

```toml
[[bin]]
name = "sqlite_args"
path = "fuzz_targets/sqlite_args.rs"
test = false
doc = false
```

Also ensure `crux-agentic` is a dependency:

```toml
[dependencies]
crux-agentic = { path = "../crates/crux-agentic" }
libfuzzer-sys = "0.4"
rusqlite = { version = "0.32", features = ["bundled"] }
tempfile = "3"
```

- [ ] **Step 3: Write the fuzz target**

Create `fuzz/fuzz_targets/sqlite_args.rs`:

```rust
#![no_main]

use crux_agentic::sqlite;
use crux_script::HandlerRegistry;
use libfuzzer_sys::fuzz_target;
use rusqlite::Connection;
use tempfile::NamedTempFile;

fuzz_target!(|data: &[u8]| {
    // Interpret fuzz input as a UTF-8 string; skip if invalid
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    // Try to parse as JSON; skip if not valid JSON
    let Ok(input) = serde_json::from_str::<serde_json::Value>(s) else {
        return;
    };

    // Set up a temp DB and register handlers
    let db_file = match NamedTempFile::new() {
        Ok(f) => f,
        Err(_) => return,
    };
    if let Ok(conn) = Connection::open(db_file.path()) {
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS t (id INTEGER PRIMARY KEY, v TEXT);",
        );
    }

    // Inject the db path into the input if args.db is missing
    let mut patched = input.clone();
    if let Some(args) = patched.get_mut("args") {
        if args.get("db").is_none() {
            args["db"] = serde_json::Value::String(
                db_file.path().to_str().unwrap_or("/tmp/fuzz.db").to_string(),
            );
        }
    }

    let mut registry = HandlerRegistry::new();
    sqlite::register(&mut registry);

    // Exercise all handlers with the fuzzed input — must never panic
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    for name in &[
        "sqlite::exec",
        "sqlite::query_many",
        "sqlite::query_one",
        "sqlite::insert",
        "sqlite::update",
        "sqlite::delete",
        "sqlite::upsert",
    ] {
        if let Some(handler) = registry.get_handler(name) {
            let _ = rt.block_on(handler(patched.clone()));
        }
    }
});
```

- [ ] **Step 4: Verify fuzz target compiles**

```bash
cd /Users/joe/dev/crux
cargo fuzz build sqlite_args
```

Expected: compiles without error (fuzzing itself is not run in CI — compile check is sufficient).

- [ ] **Step 5: Run fuzz for 10 seconds as a smoke test**

```bash
cd /Users/joe/dev/crux
cargo fuzz run sqlite_args -- -max_total_time=10
```

Expected: no panics, no crashes. `Done 1234 runs in 10 second(s)` or similar.

- [ ] **Step 6: Commit**

```bash
git -C /Users/joe/dev/crux add fuzz/
git -C /Users/joe/dev/crux commit -m "test(agentic): fuzz target for sqlite handler arg parsing"
```

---

## Task 8: Clippy + final gate

- [ ] **Step 1: Run clippy**

```bash
cd /Users/joe/dev/crux
cargo clippy -p crux-agentic -- -D warnings
```

Fix any warnings before continuing.

- [ ] **Step 2: Run full test suite**

```bash
cargo nextest run -p crux-agentic
```

Expected: all tests PASS.

- [ ] **Step 3: Commit if any clippy fixes were needed**

```bash
git -C /Users/joe/dev/crux add -A
git -C /Users/joe/dev/crux commit -m "fix(agentic): clippy fixes for sqlite module"
```

---

## Self-Review Checklist (Spec Coverage)

- [x] `sqlite::exec` — Task 3 + Task 4
- [x] `sqlite::query_one` — Task 4 (error on 0 rows, error on >1 rows, success)
- [x] `sqlite::query_many` — Task 4 (empty array, N rows)
- [x] `sqlite::insert` — Task 4 (rowid returned)
- [x] `sqlite::update` — Task 4 (rows_affected)
- [x] `sqlite::delete` — Task 4 (rows_affected)
- [x] `sqlite::upsert` — Task 4 (insert then replace)
- [x] Named params `:name` style — Task 4 `params_bind_correctly`
- [x] `bundled` rusqlite — Task 1
- [x] Constants in `handlers.rs` — Task 2
- [x] `register_all` includes sqlite — Task 6 conformance
- [x] Property tests — Task 5 (roundtrip, update, count, delete)
- [x] Fuzz target — Task 7
- [x] Clippy clean — Task 8
