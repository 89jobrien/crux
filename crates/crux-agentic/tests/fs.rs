use crux_agentic::fs as fs_handlers;
use cruxai_script::HandlerRegistry;
use serde_json::json;
use tempfile::tempdir;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    fs_handlers::register(&mut r);
    r
}

#[tokio::test]
async fn read_returns_file_content() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world").unwrap();

    let reg = registry();
    let handler = reg.get_handler("fs::read").unwrap();
    let result = handler(json!({"args": {"path": path.to_str().unwrap()}}))
        .await
        .unwrap();
    assert_eq!(result["content"].as_str().unwrap(), "hello world");
}

#[tokio::test]
async fn read_nonexistent_file_errors() {
    let reg = registry();
    let handler = reg.get_handler("fs::read").unwrap();
    let result = handler(json!({"args": {"path": "/nonexistent/path.txt"}})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn write_creates_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("out.txt");

    let reg = registry();
    let handler = reg.get_handler("fs::write").unwrap();
    let result = handler(json!({
        "args": {"path": path.to_str().unwrap(), "content": "written!"}
    }))
    .await
    .unwrap();
    assert_eq!(result["written"], true);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "written!");
}

#[tokio::test]
async fn glob_finds_files() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "").unwrap();
    std::fs::write(dir.path().join("b.rs"), "").unwrap();
    std::fs::write(dir.path().join("c.txt"), "").unwrap();

    let pattern = format!("{}/*.rs", dir.path().display());
    let reg = registry();
    let handler = reg.get_handler("fs::glob").unwrap();
    let result = handler(json!({"args": {"pattern": pattern}}))
        .await
        .unwrap();
    let paths = result["paths"].as_array().unwrap();
    assert_eq!(paths.len(), 2);
}

#[tokio::test]
async fn exists_true_for_present_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("present.txt");
    std::fs::write(&path, "").unwrap();

    let reg = registry();
    let handler = reg.get_handler("fs::exists").unwrap();
    let result = handler(json!({"args": {"path": path.to_str().unwrap()}}))
        .await
        .unwrap();
    assert_eq!(result["exists"], true);
}

#[tokio::test]
async fn exists_false_for_missing_file() {
    let reg = registry();
    let handler = reg.get_handler("fs::exists").unwrap();
    let result = handler(json!({"args": {"path": "/no/such/file.txt"}}))
        .await
        .unwrap();
    assert_eq!(result["exists"], false);
}
