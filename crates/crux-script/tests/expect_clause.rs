/// Integration tests for the `expect:` declarative post-step assertion clause (#82).
use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;

fn registry() -> Arc<HandlerRegistry> {
    let mut reg = HandlerRegistry::new();
    reg.handler_value("run_ok", |_v: Value| async {
        Ok::<Value, CruxErr>(json!({ "exit_code": 0, "stdout": "all good", "stderr": "" }))
    });
    reg.handler_value("run_fail", |_v: Value| async {
        Ok::<Value, CruxErr>(json!({ "exit_code": 1, "stdout": "", "stderr": "boom" }))
    });
    Arc::new(reg)
}

#[tokio::test]
async fn expect_exit_code_passes_when_matching() {
    let yaml = r#"
pipeline: expect_ok
steps:
  - step: run
    handler: run_ok
    expect:
      exit_code: 0
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok(), "expected success: {:?}", crux.value());
}

#[tokio::test]
async fn expect_exit_code_fails_when_mismatched() {
    let yaml = r#"
pipeline: expect_fail
steps:
  - step: run
    handler: run_fail
    expect:
      exit_code: 0
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(
        crux.value().is_err(),
        "expected failure due to expect mismatch"
    );
    let err = crux.value().unwrap_err().to_string();
    assert!(
        err.contains("exit_code"),
        "error should mention exit_code: {err}"
    );
}

#[tokio::test]
async fn expect_stdout_contains_passes() {
    let yaml = r#"
pipeline: expect_stdout
steps:
  - step: run
    handler: run_ok
    expect:
      stdout_contains: "good"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok());
}

#[tokio::test]
async fn expect_stdout_contains_fails_when_absent() {
    let yaml = r#"
pipeline: expect_stdout_fail
steps:
  - step: run
    handler: run_ok
    expect:
      stdout_contains: "nope"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_err());
}

#[tokio::test]
async fn expect_stderr_contains_checked() {
    let yaml = r#"
pipeline: expect_stderr
steps:
  - step: run
    handler: run_fail
    expect:
      stderr_contains: "boom"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok());
}

#[tokio::test]
async fn no_expect_clause_does_not_check_anything() {
    let yaml = r#"
pipeline: no_expect
steps:
  - step: run
    handler: run_fail
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok());
}
