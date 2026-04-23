/// Extended confidence test coverage.
///
/// Covers:
///   1. `handler_value` (returns plain `Value`) used with `route_on_confidence` — confidence
///      must default to 1.0, so the `[0.5, 1.0]` branch is taken.
///   2. `HandlerOutput::with_confidence` at boundary values 0.0 and 1.0.
///   3. Confidence of exactly 0.0 routes to `[0.0, 0.5)` branch.
///   4. Confidence of exactly 1.0 routes to `[0.5, 1.0]` branch.
use cruxx_script::{HandlerOutput, HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Shared pipeline — same shape reused across all tests in this file.
// ---------------------------------------------------------------------------

const ROUTE_PIPELINE: &str = r#"
pipeline: confidence_coverage
steps:
  - step: classify
    handler: classify_handler
  - route_on_confidence: route
    value: "{{ steps.classify.confidence }}"
    routes:
      - range: "[0.0, 0.5)"
        label: low
        handler: branch_low
      - range: "[0.5, 1.0]"
        label: high
        handler: branch_high
"#;

// ---------------------------------------------------------------------------
// Branch handlers (shared across tests).
// ---------------------------------------------------------------------------

async fn branch_low(_input: Value) -> Result<HandlerOutput, cruxx_core::prelude::CruxErr> {
    Ok(HandlerOutput::new(json!("branch:low")))
}

async fn branch_high(_input: Value) -> Result<HandlerOutput, cruxx_core::prelude::CruxErr> {
    Ok(HandlerOutput::new(json!("branch:high")))
}

// ---------------------------------------------------------------------------
// Registry builder.
// ---------------------------------------------------------------------------

fn registry_with<F, Fut>(classify: F) -> Arc<HandlerRegistry>
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<HandlerOutput, cruxx_core::prelude::CruxErr>>
        + Send
        + 'static,
{
    let mut reg = HandlerRegistry::new();
    reg.handler("classify_handler", classify);
    reg.handler("branch_low", branch_low);
    reg.handler("branch_high", branch_high);
    Arc::new(reg)
}

// ---------------------------------------------------------------------------
// 1. handler_value (plain Value) used with route_on_confidence — must fail with
//    a NoConfidence error because handler_value stores None, and the pipeline
//    cannot route on an absent score.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handler_value_with_route_on_confidence_returns_error() {
    // handler_value wraps the result as HandlerOutput::from(value), which sets
    // confidence = None.  When route_on_confidence tries to resolve
    // `{{ steps.classify.confidence }}` it will get ExprError::NoConfidence,
    // which the runner surfaces as a StepFailed error on the Crux trace.
    let mut reg = HandlerRegistry::new();
    reg.handler_value("classify_handler", |_: Value| async {
        Ok(json!({ "label": "plain" }))
    });
    reg.handler("branch_low", branch_low);
    reg.handler("branch_high", branch_high);

    let pipeline = load(ROUTE_PIPELINE).unwrap();
    let runner = Runner::new(Arc::new(reg));
    let crux = runner.run(&pipeline, json!({})).await;

    let err = crux.value().expect_err(
        "pipeline should fail when handler_value step is used with route_on_confidence",
    );
    let err_msg = format!("{err:?}");
    assert!(
        err_msg.contains("produced no confidence score"),
        "expected NoConfidence error, got: {err_msg}"
    );
}

// ---------------------------------------------------------------------------
// 2. Boundary: confidence exactly 0.0 → low branch [0.0, 0.5).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn confidence_zero_routes_to_low_branch() {
    let pipeline = load(ROUTE_PIPELINE).unwrap();
    let reg =
        registry_with(|_: Value| async { Ok(HandlerOutput::with_confidence(json!("zero"), 0.0)) });
    let runner = Runner::new(reg);
    let crux = runner.run(&pipeline, json!({})).await;

    let out = crux.value().expect("pipeline should succeed").clone();
    assert_eq!(
        out,
        json!("branch:low"),
        "confidence 0.0 should route to low branch, got {out}"
    );
}

// ---------------------------------------------------------------------------
// 3. Boundary: confidence exactly 1.0 → high branch [0.5, 1.0].
// ---------------------------------------------------------------------------

#[tokio::test]
async fn confidence_one_routes_to_high_branch() {
    let pipeline = load(ROUTE_PIPELINE).unwrap();
    let reg =
        registry_with(|_: Value| async { Ok(HandlerOutput::with_confidence(json!("one"), 1.0)) });
    let runner = Runner::new(reg);
    let crux = runner.run(&pipeline, json!({})).await;

    let out = crux.value().expect("pipeline should succeed").clone();
    assert_eq!(
        out,
        json!("branch:high"),
        "confidence 1.0 should route to high branch, got {out}"
    );
}

// ---------------------------------------------------------------------------
// 4. HandlerOutput::with_confidence at an explicit mid-range value (0.5).
//    0.5 is the boundary point; [0.5, 1.0] is inclusive, so → high branch.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn confidence_at_boundary_point_routes_correctly() {
    let pipeline = load(ROUTE_PIPELINE).unwrap();
    let reg =
        registry_with(|_: Value| async { Ok(HandlerOutput::with_confidence(json!("mid"), 0.5)) });
    let runner = Runner::new(reg);
    let crux = runner.run(&pipeline, json!({})).await;

    let out = crux.value().expect("pipeline should succeed").clone();
    assert_eq!(
        out,
        json!("branch:high"),
        "confidence 0.5 (inclusive lower bound of high range) should route to high branch, got {out}"
    );
}
