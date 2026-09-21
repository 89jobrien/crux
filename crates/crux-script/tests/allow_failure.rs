//! Execution tests for tolerated failures in join and pipe arms.

/// Integration tests for `allow_failure` on steps, pipe stages, and join_all arms (#80).
use crux_runtime::prelude::{CruxErr, HandlerUsage, ReplayMode, StepStatus, UsdAmount};
use crux_script::{HandlerExecution, HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn registry() -> Arc<HandlerRegistry> {
    let mut reg = HandlerRegistry::new();
    reg.handler_value("always_fails", |_v: Value| async {
        Err::<Value, CruxErr>(CruxErr::step_failed("always_fails", "boom"))
    });
    reg.handler_value("always_ok", |_v: Value| async { Ok(json!("ok")) });
    reg.handler_value("identity", |value: Value| async { Ok(value) });
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

#[tokio::test]
async fn pipe_stage_with_allow_failure_continues_to_next_stage() {
    let yaml = r#"
pipeline: tolerant_pipe
steps:
  - pipe: process
    stages:
      - step: doomed
        handler: always_fails
        allow_failure: true
      - step: inspect
        handler: identity
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;

    let result = crux.value();
    let value = result
        .as_ref()
        .expect("tolerated stage should let the pipe complete");
    assert!(value.get("error").is_some());
    let failed = crux
        .steps
        .iter()
        .find(|step| step.name == "process::doomed")
        .expect("failed stage must be traced");
    assert_eq!(failed.status, StepStatus::Err);
}

#[tokio::test]
async fn final_pipe_stage_with_allow_failure_returns_error_metadata() {
    let yaml = r#"
pipeline: tolerant_final_stage
steps:
  - pipe: process
    stages:
      - step: doomed
        handler: always_fails
        allow_failure: true
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;

    let result = crux.value();
    let value = result
        .as_ref()
        .expect("tolerated final stage should complete the pipe");
    assert_eq!(value["status"], "failed_allowed");
    assert_eq!(value["error"], "step 'always_fails' failed: boom");
}

#[tokio::test]
async fn pipe_stage_without_allow_failure_still_aborts_pipeline() {
    let yaml = r#"
pipeline: strict_pipe
steps:
  - pipe: process
    stages:
      - step: doomed
        handler: always_fails
      - step: never-runs
        handler: identity
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;

    assert!(crux.value().is_err());
    assert!(
        crux.steps
            .iter()
            .all(|step| step.name != "process::never-runs")
    );
}

#[tokio::test]
async fn tolerated_pipe_stage_usage_stops_before_next_stage_when_budget_is_exceeded() {
    let calls = Arc::new(AtomicUsize::new(0));
    let next_calls = Arc::clone(&calls);
    let mut registry = HandlerRegistry::new();
    registry.handler_metered("metered_failure", |_value| async {
        HandlerExecution::failure(
            CruxErr::step_failed("metered_failure", "boom"),
            HandlerUsage::metered(10, UsdAmount::ZERO),
        )
    });
    registry.handler_value("must_not_run", move |value| {
        let calls = Arc::clone(&next_calls);
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(value)
        }
    });
    let yaml = r#"
pipeline: budgeted_pipe
budget:
  tokens: 5
steps:
  - pipe: process
    stages:
      - step: costly-failure
        handler: metered_failure
        allow_failure: true
      - step: forbidden
        handler: must_not_run
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(registry));
    let crux = runner.run(&pipeline, json!({})).await;

    assert!(crux.value().is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn tolerated_pipe_stage_preserves_failure_as_unreported_cost_source() {
    let mut registry = HandlerRegistry::new();
    registry.handler_metered("unreported_failure", |_value| async {
        HandlerExecution::failure(
            CruxErr::step_failed("unreported_failure", "boom"),
            HandlerUsage::unreported(),
        )
    });
    let yaml = r#"
pipeline: budgeted_pipe
budget:
  usd: 1
steps:
  - pipe: process
    stages:
      - step: unreported-failure
        handler: unreported_failure
        allow_failure: true
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(registry));
    let crux = runner.run(&pipeline, json!({})).await;

    assert!(matches!(
        crux.value(),
        Err(CruxErr::UnreportedCost {
            source: Some(source),
            ..
        }) if matches!(source.as_ref(), CruxErr::StepFailed { step, .. } if step == "unreported_failure")
    ));
}

#[tokio::test]
async fn tolerated_pipe_stage_preserves_failure_as_usd_budget_source() {
    let mut registry = HandlerRegistry::new();
    registry.handler_metered("costly_failure", |_value| async {
        HandlerExecution::failure(
            CruxErr::step_failed("costly_failure", "boom"),
            HandlerUsage::metered(0, UsdAmount::from_micros(1)),
        )
    });
    let yaml = r#"
pipeline: budgeted_pipe
budget:
  usd: 0
steps:
  - pipe: process
    stages:
      - step: costly-failure
        handler: costly_failure
        allow_failure: true
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(registry));
    let crux = runner.run(&pipeline, json!({})).await;

    assert!(matches!(
        crux.value(),
        Err(CruxErr::UsdBudgetExceeded {
            source: Some(source),
            ..
        }) if matches!(source.as_ref(), CruxErr::StepFailed { step, .. } if step == "costly_failure")
    ));
}

#[tokio::test]
async fn tolerated_pipe_stage_consumes_step_budget_before_next_stage() {
    let calls = Arc::new(AtomicUsize::new(0));
    let next_calls = Arc::clone(&calls);
    let mut registry = HandlerRegistry::new();
    registry.handler_value("always_fails", |_value| async {
        Err::<Value, CruxErr>(CruxErr::step_failed("always_fails", "boom"))
    });
    registry.handler_value("must_not_run", move |value| {
        let calls = Arc::clone(&next_calls);
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(value)
        }
    });
    let yaml = r#"
pipeline: budgeted_pipe
budget:
  steps: 1
steps:
  - pipe: process
    stages:
      - step: tolerated
        handler: always_fails
        allow_failure: true
      - step: forbidden
        handler: must_not_run
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(registry));
    let crux = runner.run(&pipeline, json!({})).await;

    assert!(matches!(
        crux.value(),
        Err(CruxErr::StepBudgetExceeded {
            limit: 1,
            attempted: 2
        })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn pipe_allow_failure_does_not_swallow_strict_replay_mismatch() {
    let previous_yaml = r#"
pipeline: replay_source
steps:
  - pipe: process
    stages:
      - step: previous
        handler: always_ok
"#;
    let current_yaml = r#"
pipeline: replay_target
steps:
  - pipe: process
    stages:
      - step: current
        handler: always_fails
        allow_failure: true
"#;
    let previous_pipeline = load(previous_yaml).unwrap();
    let current_pipeline = load(current_yaml).unwrap();
    let runner = Runner::new(registry());
    let previous = runner.run(&previous_pipeline, json!({})).await;
    let replayed = runner
        .run_with_replay(&current_pipeline, json!({}), &previous, ReplayMode::Strict)
        .await;

    assert!(matches!(
        replayed.value(),
        Err(CruxErr::ReplayMismatch { .. })
    ));
}
