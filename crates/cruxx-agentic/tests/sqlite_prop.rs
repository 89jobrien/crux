//! Property-based tests for sqlite handlers using proptest.
//!
//! Properties under test:
//! 1. insert→query_one roundtrip: any valid name survives insert/read unchanged
//! 2. insert→update→query_one: updated value always reflects the new name
//! 3. insert→delete→query_many: table is always empty after delete of inserted row
//! 4. query_many count: N inserts → query_many returns exactly N rows

use cruxx_agentic::sqlite;
use cruxx_script::HandlerRegistry;
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
