#[test]
fn sqlite_constants_defined() {
    use cruxx_agentic::handlers::{
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

/// Verify that handler name constants match the strings registered by `register_all`.
use cruxx_agentic::handlers;
use cruxx_script::HandlerRegistry;

#[test]
fn handler_constants_match_registered_names() {
    let mut reg = HandlerRegistry::new();
    cruxx_agentic::register_all(&mut reg);

    let constants = [
        handlers::SHELL_EXEC,
        handlers::SHELL_CAPTURE,
        handlers::FS_READ,
        handlers::FS_WRITE,
        handlers::FS_GLOB,
        handlers::FS_EXISTS,
        handlers::GIT_STAGED_FILES,
        handlers::GIT_DIFF,
        handlers::GIT_LOG,
        handlers::GIT_STATUS,
        handlers::JSON_PICK,
        handlers::JSON_MERGE,
        handlers::JSON_JQ,
        handlers::CTRL_NOOP,
        handlers::CTRL_LOG,
        handlers::CTRL_ASSERT,
        handlers::LLM_INVOKE,
    ];

    for name in &constants {
        assert!(
            reg.get_handler(name).is_some(),
            "constant '{name}' does not match any registered handler"
        );
    }
}
