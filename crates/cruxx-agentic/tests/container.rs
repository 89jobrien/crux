use cruxx_agentic::container;
use cruxx_script::HandlerRegistry;
use serde_json::json;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    container::register(&mut r);
    r
}

#[tokio::test]
async fn container_run_handler_registered() {
    let reg = registry();
    assert!(reg.get_handler("container::run").is_some());
    assert!(reg.get_handler("container::wait").is_some());
}

#[tokio::test]
async fn container_run_returns_container_id() {
    let reg = registry();
    let handler = reg.get_handler("container::run").unwrap();
    let input = json!({
        "args": {
            "image": "alpine:latest",
            "cmd": ["echo", "hello"],
            "profile_id": "default-v1"
        }
    });
    let result = handler(input).await.unwrap();
    assert!(result.get("container_id").is_some());
}

#[tokio::test]
async fn container_wait_returns_state() {
    let reg = registry();
    let handler = reg.get_handler("container::wait").unwrap();
    let input = json!({"args": {"container_id": "mock-container-001"}});
    let result = handler(input).await.unwrap();
    assert!(result.get("state").is_some());
}
