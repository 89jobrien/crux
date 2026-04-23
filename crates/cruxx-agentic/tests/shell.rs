use cruxx_agentic::shell;
use cruxx_script::HandlerRegistry;
use serde_json::json;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    shell::register(&mut r);
    r
}

#[tokio::test]
async fn exec_runs_echo() {
    let reg = registry();
    let handler = reg.get_handler("shell::exec").unwrap();
    let result = handler(json!({"args": {"cmd": "echo hello"}}))
        .await
        .unwrap();
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["stdout"].as_str().unwrap().trim(), "hello");
}

#[tokio::test]
async fn exec_does_not_fail_on_nonzero_exit() {
    let reg = registry();
    let handler = reg.get_handler("shell::exec").unwrap();
    let result = handler(json!({"args": {"cmd": "false"}})).await.unwrap();
    assert_eq!(result["exit_code"], 1);
}

#[tokio::test]
async fn capture_succeeds_on_zero_exit() {
    let reg = registry();
    let handler = reg.get_handler("shell::capture").unwrap();
    let result = handler(json!({"args": {"cmd": "echo captured"}}))
        .await
        .unwrap();
    assert_eq!(result["stdout"].as_str().unwrap().trim(), "captured");
}

#[tokio::test]
async fn capture_fails_on_nonzero_exit() {
    let reg = registry();
    let handler = reg.get_handler("shell::capture").unwrap();
    let result = handler(json!({"args": {"cmd": "false"}})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn exec_missing_cmd_returns_error() {
    let reg = registry();
    let handler = reg.get_handler("shell::exec").unwrap();
    let result = handler(json!({})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn exec_env_injection() {
    let reg = registry();
    let handler = reg.get_handler("shell::exec").unwrap();
    let result = handler(json!({
        "args": {
            "cmd": "echo $CRUX_TEST_VAR",
            "env": { "CRUX_TEST_VAR": "injected" }
        }
    }))
    .await
    .unwrap();
    assert_eq!(result["stdout"].as_str().unwrap().trim(), "injected");
}

#[tokio::test]
async fn capture_env_injection() {
    let reg = registry();
    let handler = reg.get_handler("shell::capture").unwrap();
    let result = handler(json!({
        "args": {
            "cmd": "printenv CRUX_FILE_PATH",
            "env": { "CRUX_FILE_PATH": "/tmp/test.rs" }
        }
    }))
    .await
    .unwrap();
    assert_eq!(result["stdout"].as_str().unwrap().trim(), "/tmp/test.rs");
}

#[tokio::test]
async fn exec_env_missing_var_is_empty() {
    let reg = registry();
    let handler = reg.get_handler("shell::exec").unwrap();
    // No env injected — shell var should be empty (not an error)
    let result = handler(json!({"args": {"cmd": "echo ${CRUX_UNSET_VAR:-unset}"}}))
        .await
        .unwrap();
    assert_eq!(result["stdout"].as_str().unwrap().trim(), "unset");
}
