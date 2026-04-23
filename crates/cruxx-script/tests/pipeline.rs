/// Integration tests for YAML-driven pipeline execution.
use cruxx_script::{HandlerOutput, HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;

fn test_registry() -> Arc<HandlerRegistry> {
    let mut reg = HandlerRegistry::new();

    // Uses `handler` (not `handler_value`) so that a real confidence score of 0.85
    // is emitted, making `{{ steps.analyze.confidence }}` resolvable in routing tests.
    reg.handler("analyzer", |_input: Value| async {
        Ok::<HandlerOutput, cruxx_core::prelude::CruxErr>(HandlerOutput::with_confidence(
            json!({ "result": "analyzed" }),
            0.85,
        ))
    });

    reg.handler_value("normalize", |v: Value| async move {
        let s = v.as_str().unwrap_or("unknown");
        Ok(Value::String(s.to_uppercase()))
    });

    reg.handler_value("enrich", |v: Value| async move {
        let s = v.as_str().unwrap_or("");
        Ok(Value::String(format!("{s}_enriched")))
    });

    reg.handler_value("validate", |v: Value| async move { Ok(v) });

    reg.handler_value("web_search", |_v: Value| async { Ok(json!("web_result")) });

    reg.handler_value("local_cache", |_v: Value| async {
        Ok(json!("cache_result"))
    });

    reg.handler_value("database", |_v: Value| async { Ok(json!("db_result")) });

    reg.handler_value("fallback", |_v: Value| async { Ok(json!("fallback_out")) });
    reg.handler_value("standard", |_v: Value| async { Ok(json!("standard_out")) });
    reg.handler_value("fast_path", |_v: Value| async { Ok(json!("fast_out")) });

    reg.handler_value("conservative", |_v: Value| async {
        Ok(json!({ "strategy": "conservative", "score": 0.6 }))
    });
    reg.handler_value("aggressive", |_v: Value| async {
        Ok(json!({ "strategy": "aggressive", "score": 0.9 }))
    });
    reg.handler_value("balanced", |_v: Value| async {
        Ok(json!({ "strategy": "balanced", "score": 0.75 }))
    });

    Arc::new(reg)
}

#[tokio::test]
async fn simple_step() {
    let yaml = r#"
pipeline: simple
steps:
  - step: analyze
    handler: analyzer
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(test_registry());
    let cruxx = runner.run(&pipeline, json!("hello")).await;

    assert_eq!(cruxx.value().unwrap(), &json!({ "result": "analyzed" }));
    assert_eq!(cruxx.steps.len(), 1);
    assert_eq!(cruxx.steps[0].name, "analyze");
}

#[tokio::test]
async fn pipe_stages() {
    let yaml = r#"
pipeline: transform
steps:
  - pipe: transform
    stages:
      - normalize
      - enrich
      - validate
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(test_registry());
    let cruxx = runner.run(&pipeline, json!("hello")).await;

    assert_eq!(cruxx.value().unwrap(), &json!("HELLO_enriched"));
    assert_eq!(cruxx.steps.len(), 3);
    assert_eq!(cruxx.steps[0].name, "transform::normalize");
    assert_eq!(cruxx.steps[1].name, "transform::enrich");
    assert_eq!(cruxx.steps[2].name, "transform::validate");
}

#[tokio::test]
async fn join_all_parallel() {
    let yaml = r#"
pipeline: fetch
steps:
  - join_all: fetch_sources
    arms:
      - web_search
      - local_cache
      - database
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(test_registry());
    let cruxx = runner.run(&pipeline, json!(null)).await;

    let results = cruxx.value().unwrap();
    assert_eq!(results, &json!(["web_result", "cache_result", "db_result"]));
    assert_eq!(cruxx.steps.len(), 3);
}

#[tokio::test]
async fn route_on_confidence_routes_correctly() {
    let yaml = r#"
pipeline: classify
steps:
  - step: analyze
    handler: analyzer
  - route_on_confidence: classify
    value: "{{ steps.analyze.confidence }}"
    routes:
      - range: "[0.0, 0.5)"
        label: low
        handler: fallback
      - range: "[0.5, 0.8)"
        label: medium
        handler: standard
      - range: "[0.8, 1.0]"
        label: high
        handler: fast_path
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(test_registry());
    let cruxx = runner.run(&pipeline, json!("input")).await;

    // analyzer returns confidence 0.85 -> high route
    assert_eq!(cruxx.value().unwrap(), &json!("fast_out"));
}

#[tokio::test]
async fn speculate_pick_best() {
    let yaml = r#"
pipeline: speculate_test
steps:
  - speculate: strategies
    mode: pick_best
    arms:
      - conservative
      - aggressive
      - balanced
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(test_registry());
    let cruxx = runner.run(&pipeline, json!(null)).await;

    // aggressive has highest score (0.9)
    let result = cruxx.value().unwrap();
    assert_eq!(result["strategy"], "aggressive");
}

#[tokio::test]
async fn speculate_first_ok() {
    let yaml = r#"
pipeline: first_ok_test
steps:
  - speculate: strategies
    mode: first_ok
    arms:
      - conservative
      - aggressive
      - balanced
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(test_registry());
    let cruxx = runner.run(&pipeline, json!(null)).await;

    // first_ok returns the first successful one (conservative)
    let result = cruxx.value().unwrap();
    assert_eq!(result["strategy"], "conservative");
}

#[tokio::test]
async fn budget_is_applied() {
    let yaml = r#"
pipeline: budgeted
budget:
  tokens: 5000
steps:
  - step: analyze
    handler: analyzer
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(test_registry());
    let cruxx = runner.run(&pipeline, json!(null)).await;

    assert!(cruxx.value().is_ok());
}

#[tokio::test]
async fn multi_step_pipeline() {
    let yaml = r#"
pipeline: full
steps:
  - step: analyze
    handler: analyzer
  - join_all: fetch_sources
    arms:
      - web_search
      - database
  - pipe: transform
    stages:
      - normalize
      - enrich
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(test_registry());
    let cruxx = runner.run(&pipeline, json!("start")).await;

    // Pipeline runs sequentially: analyze -> join_all -> pipe
    // Last output is from pipe (normalize + enrich on the join_all array output)
    assert!(cruxx.value().is_ok());
    // Steps: 1 (analyze) + 2 (join arms) + 2 (pipe stages) = 5
    assert_eq!(cruxx.steps.len(), 5);
}

#[tokio::test]
async fn handler_not_found_error() {
    let yaml = r#"
pipeline: missing
steps:
  - step: bad_step
    handler: nonexistent
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(test_registry());
    let cruxx = runner.run(&pipeline, json!(null)).await;

    assert!(cruxx.value().is_err());
}

#[tokio::test]
async fn expression_input_passthrough() {
    let yaml = r#"
pipeline: expr_test
steps:
  - step: analyze
    handler: analyzer
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(test_registry());
    // The handler ignores input, but we verify the pipeline runs with arbitrary input
    let cruxx = runner.run(&pipeline, json!({"data": [1,2,3]})).await;
    assert!(cruxx.value().is_ok());
}
