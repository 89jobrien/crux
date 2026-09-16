/// Integration tests for `while:` and `repeat:` loop primitives (#89).
use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

fn registry_with_counter() -> (Arc<HandlerRegistry>, Arc<AtomicU32>) {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let mut reg = HandlerRegistry::new();
    reg.handler_value("tick", move |_v: Value| {
        let c = c.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst) + 1;
            Ok::<Value, CruxErr>(json!({ "n": n }))
        }
    });
    (Arc::new(reg), counter)
}

#[tokio::test]
async fn while_loop_runs_while_condition_true() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let mut reg = HandlerRegistry::new();
    // Condition handler: true while fewer than 3 ticks have happened so far.
    reg.handler_value("under_three", move |_v: Value| {
        let c = c.clone();
        async move { Ok::<Value, CruxErr>(json!(c.load(Ordering::SeqCst) < 3)) }
    });
    let c2 = counter.clone();
    reg.handler_value("tick", move |_v: Value| {
        let c2 = c2.clone();
        async move {
            c2.fetch_add(1, Ordering::SeqCst);
            Ok::<Value, CruxErr>(json!("ticked"))
        }
    });
    // `condition:` is checked before every iteration including the first, so an
    // initial "gate" step outside the loop seeds `steps.gate.output`; the loop's
    // own "gate" step then re-evaluates it each iteration for the next check.
    let yaml = r#"
pipeline: while_test
steps:
  - step: gate
    handler: under_three
  - while: countup
    condition: "{{ steps.gate.output }}"
    steps:
      - step: check
        handler: tick
      - step: gate
        handler: under_three
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(reg));
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok(), "{:?}", crux.value());
    assert_eq!(
        counter.load(Ordering::SeqCst),
        3,
        "should tick exactly 3 times"
    );
}

#[tokio::test]
async fn while_loop_never_runs_if_condition_starts_false() {
    let mut reg = HandlerRegistry::new();
    reg.handler_value("always_false", |_v: Value| async {
        Ok::<Value, CruxErr>(json!(false))
    });
    reg.handler_value("tick", |_v: Value| async {
        Ok::<Value, CruxErr>(json!("ticked"))
    });
    let yaml = r#"
pipeline: while_never
steps:
  - step: gate
    handler: always_false
  - while: countup
    condition: "{{ steps.gate.output }}"
    steps:
      - step: check
        handler: tick
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(reg));
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok());
    let iteration_markers: usize = crux
        .steps
        .iter()
        .filter(|s| s.name.starts_with("countup["))
        .count();
    assert_eq!(
        iteration_markers, 0,
        "condition false up front means zero iterations"
    );
}

#[tokio::test]
async fn repeat_runs_fixed_count_with_iter_index() {
    let (reg, _counter) = registry_with_counter();
    let yaml = r#"
pipeline: repeat_test
steps:
  - repeat: fixed
    count: 4
    steps:
      - step: check
        handler: tick
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(reg);
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok(), "{:?}", crux.value());
    let names: Vec<&str> = crux.steps.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"fixed[0]"));
    assert!(names.contains(&"fixed[3]"));
    assert!(!names.contains(&"fixed[4]"));
}

#[tokio::test]
async fn repeat_break_if_stops_early() {
    let mut reg = HandlerRegistry::new();
    reg.handler_value("check_reached_two", |_v: Value| async {
        Ok::<Value, CruxErr>(json!(true))
    });
    let yaml = r#"
pipeline: repeat_break
steps:
  - repeat: fixed
    count: 10
    break_if: "{{ steps.check.output }}"
    steps:
      - step: check
        handler: check_reached_two
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(reg));
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok());
    let iteration_markers: usize = crux
        .steps
        .iter()
        .filter(|s| s.name.starts_with("fixed["))
        .count();
    assert_eq!(
        iteration_markers, 1,
        "break_if true on first iteration stops immediately"
    );
}
