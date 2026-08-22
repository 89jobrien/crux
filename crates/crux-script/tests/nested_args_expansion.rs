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

#[tokio::test]
async fn join_all_arms_are_addressable_by_index() {
    // A join_all output is a positional array. Without integer segments in
    // `json_get`, no path could reach an individual arm's result.
    let yaml = r#"
pipeline: index_arm
steps:
  - join_all: fan
    arms:
      - step: first
        handler: emit_a
      - step: second
        handler: emit_b
  - step: pick
    handler: echo
    args:
      value: "{{ steps.fan.output.1 }}"
"#;
    let pipeline = load(yaml).unwrap();
    let mut registry = echo_registry();
    registry.handler_value("emit_a", |_: Value| async move { Ok(json!("A")) });
    registry.handler_value("emit_b", |_: Value| async move { Ok(json!("B")) });

    let runner = Runner::new(Arc::new(registry));
    let crux = runner.run(&pipeline, json!({})).await;

    let out = crux.value().expect("run should succeed");
    assert_eq!(
        out["seen"].as_str(),
        Some("B"),
        "index 1 must select the second arm, got: {out}"
    );
}

#[tokio::test]
async fn object_key_wins_over_array_index_lookup() {
    // A key that merely looks numeric must still resolve as a key.
    let yaml = r#"
pipeline: numeric_key
steps:
  - step: emit
    handler: emit_numeric_key
  - step: pick
    handler: echo
    args:
      value: "{{ steps.emit.output.0 }}"
"#;
    let pipeline = load(yaml).unwrap();
    let mut registry = echo_registry();
    registry.handler_value("emit_numeric_key", |_: Value| async move {
        Ok(json!({ "0": "by-key" }))
    });

    let runner = Runner::new(Arc::new(registry));
    let crux = runner.run(&pipeline, json!({})).await;

    let out = crux.value().expect("run should succeed");
    assert_eq!(out["seen"].as_str(), Some("by-key"));
}

#[tokio::test]
async fn pipe_stage_can_reference_an_earlier_stage() {
    // Stage args expand as the stage runs, against results recorded so far.
    let yaml = r#"
pipeline: stage_ref
steps:
  - pipe: chain
    stages:
      - step: produce
        handler: emit_token
      - step: consume
        handler: echo
        args:
          value: "{{ steps.produce.output.token }}"
"#;
    let pipeline = load(yaml).unwrap();
    let mut registry = echo_registry();
    registry.handler_value("emit_token", |_: Value| async move {
        Ok(json!({ "token": "from-earlier-stage" }))
    });

    let runner = Runner::new(Arc::new(registry));
    let crux = runner.run(&pipeline, json!({})).await;

    let out = crux.value().expect("pipe run should succeed");
    assert_eq!(
        out["seen"].as_str(),
        Some("from-earlier-stage"),
        "a stage must see an earlier stage's output, got: {out}"
    );
}

#[tokio::test]
async fn steps_after_a_pipe_can_reference_its_stages() {
    let yaml = r#"
pipeline: stage_lift
steps:
  - pipe: chain
    stages:
      - step: produce
        handler: emit_token
      - step: passthrough
        handler: noop
  - step: after
    handler: echo
    args:
      value: "{{ steps.produce.output.token }}"
"#;
    let pipeline = load(yaml).unwrap();
    let mut registry = echo_registry();
    registry.handler_value("emit_token", |_: Value| async move {
        Ok(json!({ "token": "lifted" }))
    });
    registry.handler_value("noop", |input: Value| async move { Ok(input) });

    let runner = Runner::new(Arc::new(registry));
    let crux = runner.run(&pipeline, json!({})).await;

    let out = crux.value().expect("run should succeed");
    assert_eq!(
        out["seen"].as_str(),
        Some("lifted"),
        "stage results must be visible after the pipe, got: {out}"
    );
}

#[tokio::test]
async fn adjacent_templates_spanning_the_whole_string_both_resolve() {
    // "{{ a }} vs {{ b }}" starts with `{{` and ends with `}}`, so a naive
    // whole-string check reads it as one template whose path is the nonsense
    // `a }} vs {{ b`. That fails to resolve and, expansion being best-effort,
    // leaves the entire string as literal text.
    let yaml = r#"
pipeline: two_templates
steps:
  - step: pick
    handler: echo
    args:
      value: "{{ input.left }} vs {{ input.right }}"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(echo_registry()));
    let crux = runner
        .run(&pipeline, json!({ "left": "A", "right": "B" }))
        .await;

    let out = crux.value().expect("run should succeed");
    assert_eq!(
        out["seen"].as_str(),
        Some("A vs B"),
        "both templates must resolve, got: {out}"
    );
}

#[tokio::test]
async fn single_whole_string_template_keeps_its_json_type() {
    // The guard above must not cost the fast path its typed return: a lone
    // whole-string template still yields the value, not its string form.
    let yaml = r#"
pipeline: typed_template
steps:
  - step: pick
    handler: echo
    args:
      value: "{{ input.payload }}"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(Arc::new(echo_registry()));
    let crux = runner
        .run(&pipeline, json!({ "payload": { "n": 42 } }))
        .await;

    let out = crux.value().expect("run should succeed");
    assert_eq!(
        out["seen"]["n"].as_i64(),
        Some(42),
        "a lone template must return the typed value, got: {out}"
    );
}
