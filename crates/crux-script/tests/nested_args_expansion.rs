//! Template expansion inside combinator args.
//!
//! `{{ ... }}` templates were originally expanded only for plain `step:` args.
//! Args nested inside `join_all` arms, `pipe` stages and `route_on_confidence`
//! branches reached their handler as the literal template string, which made
//! any pipeline that parameterised a combinator silently wrong. These tests
//! pin the expansion at each of the three nested sites.

use crux_script::{HandlerOutput, HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;

/// Echoes back whatever landed in `args.value`, so a test can tell an expanded
/// template from an unexpanded one.
fn echo_registry() -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();
    registry.handler_value("echo", |input: Value| async move {
        Ok(json!({ "seen": input["args"]["value"].clone() }))
    });
    registry
}

#[tokio::test]
async fn join_all_arm_args_expand_templates() {
    let yaml = r#"
pipeline: join_expand
steps:
  - join_all: fan
    arms:
      - step: left
        handler: echo
        args:
          value: "{{ input.subject }}"
      - step: right
        handler: echo
        args:
          value: "literal"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(echo_registry()));
    let crux = runner
        .run(&pipeline, json!({ "subject": "expanded" }))
        .await;

    let out = crux.value().expect("join_all run should succeed");
    assert_eq!(
        out[0]["seen"].as_str(),
        Some("expanded"),
        "arm args must be template-expanded, got: {out}"
    );
    assert_eq!(
        out[1]["seen"].as_str(),
        Some("literal"),
        "non-template arm args must pass through untouched"
    );
}

#[tokio::test]
async fn pipe_stage_args_expand_templates() {
    let yaml = r#"
pipeline: pipe_expand
steps:
  - pipe: chain
    stages:
      - step: only
        handler: echo
        args:
          value: "{{ input.subject }}"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(echo_registry()));
    let crux = runner
        .run(&pipeline, json!({ "subject": "expanded" }))
        .await;

    let out = crux.value().expect("pipe run should succeed");
    assert_eq!(
        out["seen"].as_str(),
        Some("expanded"),
        "pipe stage args must be template-expanded, got: {out}"
    );
}

#[tokio::test]
async fn route_branch_args_expand_templates() {
    let yaml = r#"
pipeline: route_expand
steps:
  - step: score
    handler: scorer
  - route_on_confidence: pick
    value: "{{ steps.score.confidence }}"
    routes:
      - range: "[0.0, 0.5)"
        label: low
        handler: echo
        args:
          value: "{{ input.subject }}"
      - range: "[0.5, 1.0]"
        label: high
        handler: echo
        args:
          value: "{{ input.subject }}"
"#;
    let pipeline = load(yaml).unwrap();
    let mut registry = echo_registry();
    registry.handler("scorer", |_input: Value| async move {
        Ok(HandlerOutput::with_confidence(json!({}), 0.9))
    });

    let runner = Runner::new(Arc::new(registry));
    let crux = runner
        .run(&pipeline, json!({ "subject": "expanded" }))
        .await;

    let out = crux.value().expect("route run should succeed");
    assert_eq!(
        out["seen"].as_str(),
        Some("expanded"),
        "route branch args must be template-expanded, got: {out}"
    );
}

#[tokio::test]
async fn unresolvable_template_is_left_intact() {
    // Expansion is best-effort: an unknown path keeps the original string
    // rather than failing the step, so static pipelines need no ExprContext.
    let yaml = r#"
pipeline: join_unknown
steps:
  - join_all: fan
    arms:
      - step: only
        handler: echo
        args:
          value: "{{ input.missing }}"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(echo_registry()));
    let crux = runner.run(&pipeline, json!({})).await;

    let out = crux
        .value()
        .expect("run should succeed despite unknown path");
    assert_eq!(
        out[0]["seen"].as_str(),
        Some("{{ input.missing }}"),
        "an unresolvable template must survive as its literal text"
    );
}
