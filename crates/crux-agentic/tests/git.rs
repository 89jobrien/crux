use crux_agentic::git;
use cruxai_script::HandlerRegistry;
use serde_json::json;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    git::register(&mut r);
    r
}

#[tokio::test]
async fn staged_files_returns_array() {
    let reg = registry();
    let handler = reg.get_handler("git::staged_files").unwrap();
    let result = handler(json!({})).await.unwrap();
    assert!(result["files"].is_array());
}

#[tokio::test]
async fn status_returns_clean_field() {
    let reg = registry();
    let handler = reg.get_handler("git::status").unwrap();
    let result = handler(json!({})).await.unwrap();
    assert!(result["clean"].is_boolean());
    assert!(result["porcelain"].is_string());
}

#[tokio::test]
async fn log_returns_commits() {
    let reg = registry();
    let handler = reg.get_handler("git::log").unwrap();
    let result = handler(json!({"args": {"n": 3}})).await.unwrap();
    let commits = result["commits"].as_array().unwrap();
    assert!(!commits.is_empty());
    let first = &commits[0];
    assert!(first["hash"].is_string());
    assert!(first["subject"].is_string());
}

#[tokio::test]
async fn diff_returns_string() {
    let reg = registry();
    let handler = reg.get_handler("git::diff").unwrap();
    let result = handler(json!({})).await.unwrap();
    assert!(result["diff"].is_string());
}
