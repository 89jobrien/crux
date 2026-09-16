/// Integration tests for `on_error:` step-level recovery (#88).
use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;

fn registry() -> Arc<HandlerRegistry> {
    let mut reg = HandlerRegistry::new();
    reg.handler_value("always_fails", |_v: Value| async {
        Err::<Value, CruxErr>(CruxErr::step_failed("always_fails", "nope"))
    });
    reg.handler_value("cleanup", |_v: Value| async { Ok(json!("cleaned_up")) });
    reg.handler_value("also_fails", |_v: Value| async {
        Err::<Value, CruxErr>(CruxErr::step_failed("also_fails", "still broken"))
    });
    Arc::new(reg)
}

#[tokio::test]
async fn on_error_recovers_and_pipeline_succeeds() {
    let yaml = r#"
pipeline: recover
steps:
  - step: run
    handler: always_fails
    on_error:
      handler: cleanup
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(
        crux.value().is_ok(),
        "on_error should recover: {:?}",
        crux.value()
    );
    assert_eq!(crux.value().unwrap(), &json!("cleaned_up"));
}

#[tokio::test]
async fn on_error_failing_without_allow_failure_propagates() {
    let yaml = r#"
pipeline: recover_fails
steps:
  - step: run
    handler: always_fails
    on_error:
      handler: also_fails
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_err());
}

#[tokio::test]
async fn on_error_failing_with_allow_failure_tolerated() {
    let yaml = r#"
pipeline: recover_fails_tolerated
steps:
  - step: run
    handler: always_fails
    allow_failure: true
    on_error:
      handler: also_fails
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok());
}

#[tokio::test]
async fn on_error_runs_after_retries_exhausted() {
    let yaml = r#"
pipeline: recover_after_retry
steps:
  - step: run
    handler: always_fails
    retry:
      count: 2
      delay_ms: 1
    on_error:
      handler: cleanup
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok());
    assert_eq!(crux.value().unwrap(), &json!("cleaned_up"));
    // 3 retry attempts + 1 on_error step = 4 traced steps.
    assert_eq!(crux.steps.len(), 4);
}

#[tokio::test]
async fn no_on_error_field_still_propagates_normally() {
    let yaml = r#"
pipeline: no_on_error
steps:
  - step: run
    handler: always_fails
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_err());
}
