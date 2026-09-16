/// Integration tests for per-step `retry` with backoff (#79).
use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

fn registry_with_counter() -> (Arc<HandlerRegistry>, Arc<AtomicU32>) {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let mut reg = HandlerRegistry::new();
    reg.handler_value("fail_twice_then_ok", move |_v: Value| {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err::<Value, CruxErr>(CruxErr::step_failed("fail_twice_then_ok", "not yet"))
            } else {
                Ok(json!("recovered"))
            }
        }
    });
    reg.handler_value("always_fails", move |_v: Value| async {
        Err::<Value, CruxErr>(CruxErr::step_failed("always_fails", "nope"))
    });
    (Arc::new(reg), counter)
}

#[tokio::test]
async fn retry_recovers_after_transient_failures() {
    let (reg, counter) = registry_with_counter();
    let yaml = r#"
pipeline: retry_ok
steps:
  - step: run
    handler: fail_twice_then_ok
    retry:
      count: 3
      delay_ms: 1
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(reg);
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(
        crux.value().is_ok(),
        "expected eventual success: {:?}",
        crux.value()
    );
    assert_eq!(crux.value().unwrap(), &json!("recovered"));
    assert_eq!(
        counter.load(Ordering::SeqCst),
        3,
        "should have attempted 3 times total"
    );
}

#[tokio::test]
async fn retry_exhausted_propagates_last_error() {
    let (reg, _counter) = registry_with_counter();
    let yaml = r#"
pipeline: retry_exhausted
steps:
  - step: run
    handler: always_fails
    retry:
      count: 2
      delay_ms: 1
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(reg);
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_err());
}

#[tokio::test]
async fn each_retry_attempt_is_a_traced_sub_step() {
    let (reg, _counter) = registry_with_counter();
    let yaml = r#"
pipeline: retry_traced
steps:
  - step: run
    handler: always_fails
    retry:
      count: 2
      delay_ms: 1
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(reg);
    let crux = runner.run(&pipeline, json!({})).await;
    // 1 initial + 2 retries = 3 attempts, each traced.
    assert_eq!(
        crux.steps.len(),
        3,
        "expected 3 traced attempt steps, got {:?}",
        crux.steps.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn no_retry_field_fails_immediately() {
    let (reg, _counter) = registry_with_counter();
    let yaml = r#"
pipeline: no_retry
steps:
  - step: run
    handler: always_fails
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(reg);
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_err());
    assert_eq!(
        crux.steps.len(),
        1,
        "no retry means a single traced attempt"
    );
}
