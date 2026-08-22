/// Integration tests for the `poll:` do-while loop (#83).
use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

fn registry_with_counter() -> (Arc<HandlerRegistry>, Arc<AtomicU32>) {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let mut reg = HandlerRegistry::new();
    reg.handler_value("check_ready", move |_v: Value| {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst) + 1;
            Ok::<Value, CruxErr>(json!({ "attempt": n, "ready": n >= 3 }))
        }
    });
    (Arc::new(reg), counter)
}

#[tokio::test]
async fn poll_runs_at_least_once_and_stops_when_until_is_true() {
    let (reg, counter) = registry_with_counter();
    let yaml = r#"
pipeline: poll_ok
steps:
  - poll: wait_ready
    until: "{{ steps.check.output.ready }}"
    interval_ms: 1
    steps:
      - step: check
        handler: check_ready
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(reg);
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok(), "{:?}", crux.value());
    assert_eq!(
        counter.load(Ordering::SeqCst),
        3,
        "should poll exactly 3 times"
    );
}

#[tokio::test]
async fn poll_stops_at_max_attempts_even_if_never_ready() {
    let mut reg = HandlerRegistry::new();
    reg.handler_value("never_ready", |_v: Value| async {
        Ok::<Value, CruxErr>(json!({ "ready": false }))
    });
    let yaml = r#"
pipeline: poll_capped
steps:
  - poll: wait_ready
    until: "{{ steps.check.output.ready }}"
    max_attempts: 2
    interval_ms: 1
    steps:
      - step: check
        handler: never_ready
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(reg));
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok());
    // 2 poll iterations, each running 1 inner step + 1 iteration marker = 4 traced steps.
    assert_eq!(
        crux.steps.len(),
        4,
        "expected 2 iterations x (1 inner step + 1 marker)"
    );
}

#[tokio::test]
async fn poll_each_iteration_is_a_traced_sub_step() {
    let (reg, _counter) = registry_with_counter();
    let yaml = r#"
pipeline: poll_traced
steps:
  - poll: wait_ready
    until: "{{ steps.check.output.ready }}"
    interval_ms: 1
    steps:
      - step: check
        handler: check_ready
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(reg);
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok());
    let names: Vec<&str> = crux.steps.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"wait_ready[0]"),
        "expected an iteration marker step, got {names:?}"
    );
}
