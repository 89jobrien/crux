/// Integration tests for per-step `timeout_ms` (#81).
use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

fn registry() -> Arc<HandlerRegistry> {
    let mut reg = HandlerRegistry::new();
    reg.handler_value("slow", |_v: Value| async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok::<Value, CruxErr>(json!("done"))
    });
    reg.handler_value("fast", |_v: Value| async { Ok(json!("done")) });
    Arc::new(reg)
}

#[tokio::test]
async fn step_exceeding_timeout_fails() {
    let yaml = r#"
pipeline: too_slow
steps:
  - step: run
    handler: slow
    timeout_ms: 20
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_err(), "expected timeout failure");
    let err = crux.value().unwrap_err().to_string();
    assert!(
        err.contains("timed out") || err.contains("timeout"),
        "error should mention timeout: {err}"
    );
}

#[tokio::test]
async fn step_within_timeout_succeeds() {
    let yaml = r#"
pipeline: fast_enough
steps:
  - step: run
    handler: fast
    timeout_ms: 5000
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok());
    assert_eq!(crux.value().unwrap(), &json!("done"));
}

#[tokio::test]
async fn no_timeout_field_never_times_out() {
    let yaml = r#"
pipeline: no_timeout
steps:
  - step: run
    handler: slow
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok());
}
