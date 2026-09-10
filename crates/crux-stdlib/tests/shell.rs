use crux_script::HandlerRegistry;
use serde_json::json;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    crux_stdlib::shell::register(&mut r);
    r
}

#[tokio::test]
async fn shell_capture_success_reports_zero_usd() {
    let reg = registry();
    let handler = reg.get_handler("shell::capture").unwrap();
    let execution = handler(json!({ "args": { "cmd": "echo hello" } })).await;
    assert_eq!(execution.usage.usd.map(|amount| amount.micros()), Some(0));
    let result = execution.outcome.unwrap().value;
    assert_eq!(result["stdout"].as_str().unwrap().trim(), "hello");
    assert_eq!(result["exit_code"], 0);
}

#[tokio::test]
async fn shell_exec_nonzero_does_not_fail() {
    let reg = registry();
    let handler = reg.get_handler("shell::exec").unwrap();
    let result = handler(json!({ "args": { "cmd": "exit 42" } }))
        .await
        .outcome
        .unwrap()
        .value;
    assert_eq!(result["exit_code"], 42);
}

#[tokio::test]
async fn shell_capture_failure_reports_zero_usd() {
    let reg = registry();
    let handler = reg.get_handler("shell::capture").unwrap();
    let execution = handler(json!({ "args": { "cmd": "exit 1" } })).await;
    assert_eq!(execution.usage.usd.map(|amount| amount.micros()), Some(0));
    assert!(execution.outcome.is_err());
}

#[tokio::test]
async fn shell_capture_ignore_exit() {
    let reg = registry();
    let handler = reg.get_handler("shell::capture").unwrap();
    let result = handler(json!({ "args": { "cmd": "exit 7", "ignore_exit": true } }))
        .await
        .outcome
        .unwrap()
        .value;
    assert_eq!(result["exit_code"], 7);
}

#[tokio::test]
async fn shell_capture_with_cwd() {
    let reg = registry();
    let handler = reg.get_handler("shell::capture").unwrap();
    let result = handler(json!({ "args": { "cmd": "pwd", "cwd": "/tmp" } }))
        .await
        .outcome
        .unwrap()
        .value;
    let stdout = result["stdout"].as_str().unwrap().trim();
    assert!(stdout.contains("tmp"), "expected /tmp, got {stdout}");
}
