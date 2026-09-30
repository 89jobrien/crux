//! Runtime tests for `for_each` iteration bindings and early exit.

/// Integration tests for the `for_each:` loop (#84).
use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn registry() -> Arc<HandlerRegistry> {
    let mut reg = HandlerRegistry::new();
    reg.handler_value("double_item", |input: Value| async move {
        let n = input
            .get("args")
            .and_then(|a| a.get("value"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        Ok::<Value, CruxErr>(json!(n * 2))
    });
    Arc::new(reg)
}

#[tokio::test]
async fn for_each_runs_once_per_item_with_iter_bindings() {
    let yaml = r#"
pipeline: map_over
vars:
  numbers: "{{ input.numbers }}"
steps:
  - for_each: doubles as n
    items: "{{ input.numbers }}"
    steps:
      - step: doubled
        handler: double_item
        args:
          value: "{{ iter.n }}"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({ "numbers": [1, 2, 3] })).await;
    assert!(crux.value().is_ok(), "{:?}", crux.value());
    assert_eq!(
        crux.value().unwrap(),
        &json!(6),
        "last iteration's output wins"
    );

    let names: Vec<&str> = crux.steps.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"doubles[0]"));
    assert!(names.contains(&"doubles[1]"));
    assert!(names.contains(&"doubles[2]"));
}

#[tokio::test]
async fn for_each_break_if_stops_early() {
    let mut reg = HandlerRegistry::new();
    reg.handler_value("is_over_two", |input: Value| async move {
        let n = input
            .get("args")
            .and_then(|a| a.get("value"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        Ok::<Value, CruxErr>(json!(n > 2))
    });
    let yaml = r#"
pipeline: break_early
steps:
  - for_each: doubles as n
    items: "{{ input.numbers }}"
    break_if: "{{ steps.check.output }}"
    steps:
      - step: check
        handler: is_over_two
        args:
          value: "{{ iter.n }}"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(reg));
    let crux = runner
        .run(&pipeline, json!({ "numbers": [1, 2, 3, 4, 5] }))
        .await;
    assert!(crux.value().is_ok(), "{:?}", crux.value());
    // Stops once check(n) > 2 is true, i.e. at n=3 (index 2) — iterations 0,1,2 ran.
    let iteration_markers: usize = crux
        .steps
        .iter()
        .filter(|s| s.name.starts_with("doubles["))
        .count();
    assert_eq!(
        iteration_markers, 3,
        "should stop after breaking at index 2"
    );
}

#[tokio::test]
async fn for_each_over_empty_array_never_runs_body() {
    let yaml = r#"
pipeline: empty_loop
steps:
  - for_each: doubles as n
    items: "{{ input.numbers }}"
    steps:
      - step: doubled
        handler: double_item
        args:
          value: "{{ iter.n }}"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({ "numbers": [] })).await;
    assert!(crux.value().is_ok());
    assert_eq!(crux.steps.len(), 0, "empty items means zero traced steps");
}

#[tokio::test]
async fn parallel_for_each_bounds_concurrency_and_merges_traces_in_item_order() {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let mut reg = HandlerRegistry::new();
    let active_handler = Arc::clone(&active);
    let peak_handler = Arc::clone(&peak);
    reg.handler_value("delayed", move |input: Value| {
        let active = Arc::clone(&active_handler);
        let peak = Arc::clone(&peak_handler);
        async move {
            let running = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(running, Ordering::SeqCst);
            let value = input["args"]["value"].as_u64().unwrap();
            tokio::time::sleep(std::time::Duration::from_millis((4 - value) * 15)).await;
            active.fetch_sub(1, Ordering::SeqCst);
            Ok::<Value, CruxErr>(json!(value))
        }
    });

    let pipeline = load(
        r#"
pipeline: parallel_map
steps:
  - for_each: items as item
    items: "{{ input.items }}"
    parallel: true
    max_concurrency: 2
    steps:
      - step: work
        handler: delayed
        args:
          value: "{{ iter.item }}"
"#,
    )
    .unwrap();
    let crux = Runner::new(Arc::new(reg))
        .run(&pipeline, json!({ "items": [1, 2, 3] }))
        .await;

    assert_eq!(peak.load(Ordering::SeqCst), 2);
    assert_eq!(crux.value().unwrap(), &json!(3));
    let names: Vec<_> = crux.steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["work", "items[0]", "work", "items[1]", "work", "items[2]"]
    );
}
