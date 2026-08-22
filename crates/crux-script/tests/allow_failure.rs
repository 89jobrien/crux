/// Integration tests for `allow_failure` on steps and join_all arms (#80).
use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;

fn registry() -> Arc<HandlerRegistry> {
    let mut reg = HandlerRegistry::new();
    reg.handler_value("always_fails", |_v: Value| async {
        Err::<Value, CruxErr>(CruxErr::step_failed("always_fails", "boom"))
    });
    reg.handler_value("always_ok", |_v: Value| async { Ok(json!("ok")) });
    Arc::new(reg)
}

#[tokio::test]
async fn step_without_allow_failure_aborts_pipeline() {
    let yaml = r#"
pipeline: strict
steps:
  - step: doomed
    handler: always_fails
  - step: never_runs
    handler: always_ok
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_err());
}

#[tokio::test]
async fn step_with_allow_failure_continues_pipeline() {
    let yaml = r#"
pipeline: tolerant
steps:
  - step: doomed
    handler: always_fails
    allow_failure: true
  - step: after
    handler: always_ok
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(
        crux.value().is_ok(),
        "pipeline should continue past an allow_failure step: {:?}",
        crux.value()
    );
    assert_eq!(crux.value().unwrap(), &json!("ok"));
}

#[tokio::test]
async fn join_all_without_allow_failure_fails_whole_join() {
    let yaml = r#"
pipeline: strict_join
steps:
  - join_all: fan
    arms:
      - step: a
        handler: always_ok
      - step: b
        handler: always_fails
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_err());
}

#[tokio::test]
async fn join_all_arm_with_allow_failure_completes_with_partial_results() {
    let yaml = r#"
pipeline: tolerant_join
steps:
  - join_all: fan
    arms:
      - step: a
        handler: always_ok
      - step: b
        handler: always_fails
        allow_failure: true
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(
        crux.value().is_ok(),
        "join_all should tolerate an allow_failure arm: {:?}",
        crux.value()
    );
    let arr = crux.value().unwrap().as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0], json!("ok"));
    assert!(
        arr[1].get("error").is_some(),
        "failed arm should carry error metadata"
    );
}
