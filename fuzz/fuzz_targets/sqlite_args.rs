#![no_main]

use crux_agentic::sqlite;
use crux_script::HandlerRegistry;
use libfuzzer_sys::fuzz_target;
use rusqlite::Connection;
use tempfile::NamedTempFile;

fuzz_target!(|data: &[u8]| {
    // Interpret fuzz input as UTF-8; skip if invalid
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    // Try to parse as JSON; skip if not valid JSON
    let Ok(input) = serde_json::from_str::<serde_json::Value>(s) else {
        return;
    };

    // Set up a temp DB
    let db_file = match NamedTempFile::new() {
        Ok(f) => f,
        Err(_) => return,
    };
    if let Ok(conn) = Connection::open(db_file.path()) {
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS t (id INTEGER PRIMARY KEY, v TEXT);",
        );
    }

    // Inject db path into input if args.db is missing
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
