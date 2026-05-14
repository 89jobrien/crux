/// Integration test: handler confidence emission for route_on_confidence.
///
/// Verifies that a handler returning `HandlerOutput::with_confidence(value, score)` has its
/// confidence score threaded through to the `ExprContext`, making it available to a subsequent
/// `route_on_confidence` step via `{{ steps.<name>.confidence }}`.
use cruxx_script::{HandlerOutput, HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;

/// A handler that emits a low confidence score (0.3).
async fn low_confidence_handler(
    _input: Value,
) -> Result<HandlerOutput, cruxx_core::prelude::CruxErr> {
    Ok(HandlerOutput::with_confidence(
        json!({ "data": "low" }),
        0.3,
    ))
}

/// A handler that emits a high confidence score (0.9).
async fn high_confidence_handler(
    _input: Value,
) -> Result<HandlerOutput, cruxx_core::prelude::CruxErr> {
    Ok(HandlerOutput::with_confidence(
        json!({ "data": "high" }),
        0.9,
    ))
}

/// Route taken when confidence is in [0.0, 0.5) — the "low" branch.
async fn branch_low(_input: Value) -> Result<HandlerOutput, cruxx_core::prelude::CruxErr> {
    Ok(HandlerOutput::new(json!("branch:low")))
}

/// Route taken when confidence is in [0.5, 1.0] — the "high" branch.
async fn branch_high(_input: Value) -> Result<HandlerOutput, cruxx_core::prelude::CruxErr> {
    Ok(HandlerOutput::new(json!("branch:high")))
}

const PIPELINE_YAML: &str = r#"
pipeline: confidence_routing_test
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

#[allow(clippy::type_complexity)]
fn make_registry(
    classify: fn(
        Value,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<HandlerOutput, cruxx_core::prelude::CruxErr>>
                + Send,
        >,
    >,
) -> Arc<HandlerRegistry> {
    let mut reg = HandlerRegistry::new();
    reg.handler("classify_handler", classify);
    reg.handler("branch_low", branch_low);
    reg.handler("branch_high", branch_high);
    Arc::new(reg)
}

#[tokio::test]
async fn low_confidence_routes_to_low_branch() {
    let pipeline = load(PIPELINE_YAML).unwrap();
    let reg = make_registry(|v| Box::pin(low_confidence_handler(v)));
    let runner = Runner::new(reg);
    let crux = runner.run(&pipeline, json!({})).await;

    let out = crux.value().expect("pipeline should succeed").clone();
    assert_eq!(
        out,
        json!("branch:low"),
        "confidence 0.3 should route to low branch, got {out}"
    );
}

#[tokio::test]
async fn high_confidence_routes_to_high_branch() {
    let pipeline = load(PIPELINE_YAML).unwrap();
    let reg = make_registry(|v| Box::pin(high_confidence_handler(v)));
    let runner = Runner::new(reg);
    let crux = runner.run(&pipeline, json!({})).await;

    let out = crux.value().expect("pipeline should succeed").clone();
    assert_eq!(
        out,
        json!("branch:high"),
        "confidence 0.9 should route to high branch, got {out}"
    );
}
