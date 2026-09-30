//! Integration tests for shell exit behavior, working directories, and free usage.

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
    let execution = handler(json!({ "args": {} })).await;
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

#[tokio::test]
async fn process_run_passes_untrusted_arguments_literally() {
    let reg = registry();
    let handler = reg.get_handler("process::run").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("injected");
    let untrusted = format!("$(touch {})", marker.display());
    let result = handler(json!({
        "args": {
            "program": "printf",
            "argv": ["%s", untrusted]
        }
    }))
    .await
    .outcome
    .unwrap()
    .value;

    assert_eq!(result["stdout"], untrusted);
    assert!(!marker.exists(), "argument was interpreted by a shell");
}

#[tokio::test]
async fn process_run_supports_cwd_env_and_nonzero_policy() {
    let reg = registry();
    let handler = reg.get_handler("process::run").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let canonical_temp = temp.path().canonicalize().unwrap();
    let result = handler(json!({
        "args": {
            "program": "sh",
            "argv": ["-c", "printf '%s:%s' \"$PWD\" \"$CRUX_TYPED\"; exit 7"],
            "cwd": temp.path(),
            "env": {"CRUX_TYPED": "yes"},
            "fail_on_nonzero": false
        }
    }))
    .await
    .outcome
    .unwrap()
    .value;

    assert_eq!(result["exit_code"], 7);
    assert_eq!(
        result["stdout"],
        format!("{}:yes", canonical_temp.display())
    );
}
