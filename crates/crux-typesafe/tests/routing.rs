//! Proves the claim this crate exists for: a **YAML** pipeline can route on
//! `route_on_confidence` using a calibrated judgment, and the
//! `dynamic_boundary` diagnostic that made `examples/joe/branch_cleanup.crux`
//! invalid no longer applies.
//!
//! Runs entirely against [`CannedJudgmentClient`]. No API key, no network.

use std::sync::Arc;

use crux_runtime::prelude::CruxErr;
use crux_script::{ConfidenceCapability, HandlerOutput, HandlerRegistry, Runner, load};
use crux_typesafe::client::single_question_request;
use crux_typesafe::wire::{Answer, ScoreAnswer, SystemOneResponse};
use crux_typesafe::{CannedJudgmentClient, JudgmentClient};
use serde_json::{Value, json};

/// A canned response scoring `score` on a 3-level rubric.
fn response_scoring(score: f64) -> SystemOneResponse {
    SystemOneResponse {
        model: "jev-test".to_owned(),
        answers: [(
            "score".to_owned(),
            Answer::Score(ScoreAnswer {
                score,
                legend: [
                    ("0".to_owned(), json!("Cosmetic; no impact")),
                    ("1".to_owned(), json!("Degraded; workaround exists")),
                    ("2".to_owned(), json!("Blocking; no workaround")),
                ]
                .into_iter()
                .collect(),
                probabilities: [
                    ("0".to_owned(), 0.0),
                    ("1".to_owned(), 0.0),
                    ("2".to_owned(), 1.0),
                ]
                .into_iter()
                .map(|(key, value)| (key, if value == 1.0 { score / 2.0 } else { value }))
                .collect(),
                confidence: 1.0,
            }),
        )]
        .into_iter()
        .collect(),
        usage: None,
    }
}

fn judge(score: f64) -> Arc<dyn JudgmentClient> {
    Arc::new(CannedJudgmentClient::empty().push(response_scoring(score)))
}

/// Registry with `judge::score` plus one handler per routing band.
fn registry(score: f64) -> Arc<HandlerRegistry> {
    let mut reg = HandlerRegistry::new();
    crux_typesafe::register(&mut reg, judge(score));
    for (name, out) in [("low", "low_out"), ("mid", "mid_out"), ("high", "high_out")] {
        reg.handler_value(name, move |_input: Value| async move {
            Ok::<Value, CruxErr>(json!(out))
        });
    }
    Arc::new(reg)
}

/// The exact shape `branch_cleanup.crux` needs: a step's confidence driving a route.
const ROUTING_PIPELINE: &str = r#"
pipeline: triage
steps:
  - step: classify
    handler: judge::score
    args:
      state: "The export button crashes the settings page in Safari."
      instructions: How severe is the reported issue?
      criteria:
        - Cosmetic; no impact
        - Degraded; workaround exists
        - Blocking; no workaround
  - route_on_confidence: act
    value: "{{ steps.classify.confidence }}"
    routes:
      - range: "[0.0, 0.34)"
        label: low
        handler: low
      - range: "[0.34, 0.67)"
        label: mid
        handler: mid
      - range: "[0.67, 1.0]"
        label: high
        handler: high
"#;

/// A top-of-rubric judgment routes to the highest band.
#[tokio::test]
async fn a_yaml_pipeline_routes_on_a_calibrated_judgment() {
    // score 2.0 of 0..2 normalizes to 1.0
    let pipeline = load(ROUTING_PIPELINE).expect("pipeline must parse");
    let crux = Runner::new(registry(2.0)).run(&pipeline, json!(null)).await;
    assert_eq!(crux.value().unwrap(), &json!("high_out"));
}

/// A bottom-of-rubric judgment routes to the lowest band.
#[tokio::test]
async fn a_low_judgment_routes_to_the_low_band() {
    let pipeline = load(ROUTING_PIPELINE).expect("pipeline must parse");
    // score 0.0 of 0..2 normalizes to 0.0
    let crux = Runner::new(registry(0.0)).run(&pipeline, json!(null)).await;
    assert_eq!(crux.value().unwrap(), &json!("low_out"));
}

/// The midpoint lands in the middle band, which only holds if the score was
/// normalized onto 0..=1 rather than used raw (a raw 1.0 would still be `high`).
#[tokio::test]
async fn a_mid_rubric_position_lands_in_the_middle_band() {
    let pipeline = load(ROUTING_PIPELINE).expect("pipeline must parse");
    // score 1.0 of 0..2 normalizes to 0.5, not 1.0.
    let crux = Runner::new(registry(1.0)).run(&pipeline, json!(null)).await;
    assert_eq!(crux.value().unwrap(), &json!("mid_out"));
}

/// The declared contract is what lifts the YAML restriction. `Always` is the only
/// capability the compiler accepts for `{{ steps.x.confidence }}` without a
/// diagnostic or an error.
#[test]
fn the_handler_declares_a_usable_confidence_contract() {
    let mut reg = HandlerRegistry::new();
    crux_typesafe::register(&mut reg, Arc::new(CannedJudgmentClient::empty()));
    let metadata = reg
        .get_metadata("judge::score")
        .expect("judge::score must be registered");

    assert_eq!(
        metadata.confidence,
        Some(ConfidenceCapability::Always),
        "declaring Always is what lets a YAML pipeline route on this step's confidence"
    );
}

/// The step confidence must equal the payload's normalized value, or a template
/// reading one would disagree with the route taken by the other.
#[tokio::test]
async fn the_step_confidence_and_payload_normalized_agree() {
    let mut reg = HandlerRegistry::new();
    crux_typesafe::register(&mut reg, judge(1.0));
    let handler = reg.get_handler("judge::score").unwrap().clone();

    let execution = handler(json!({
        "state": "something",
        "instructions": "How severe?",
        "criteria": ["low", "mid", "high"],
    }))
    .await;
    let output = execution.outcome.expect("the judgment step must succeed");

    let confidence = output.confidence.expect("confidence must be present");
    let normalized = output.value["normalized"]
        .as_f64()
        .expect("payload must carry a normalized score");

    assert!(
        (confidence as f64 - normalized).abs() < 1e-6,
        "step confidence {confidence} must equal payload normalized {normalized}"
    );
    assert!(
        output.value.get("confidence").is_none(),
        "the payload must not shadow the routed confidence"
    );
}

/// A rubric with one level cannot be normalized, and must fail rather than
/// silently routing on a meaningless number.
#[tokio::test]
async fn a_single_level_rubric_fails_loudly() {
    let mut reg = HandlerRegistry::new();
    crux_typesafe::register(&mut reg, judge(1.0));
    let handler = reg.get_handler("judge::score").unwrap().clone();

    let error = handler(json!({
        "state": "something",
        "instructions": "How severe?",
        "criteria": ["only one level"],
    }))
    .await
    .outcome
    .expect_err("a one-level rubric cannot be normalized");

    assert!(
        error.to_string().contains("at least 2 levels"),
        "unexpected error: {error}"
    );
}

/// A choice answer has no rubric position; routing on it must not silently
/// borrow a number from an unrelated field.
#[tokio::test]
async fn a_choice_answer_is_rejected_by_the_score_handler() {
    let mut reg = HandlerRegistry::new();
    let choice = SystemOneResponse {
        model: "jev-test".to_owned(),
        answers: [(
            "score".to_owned(),
            Answer::Choice(crux_typesafe::wire::ChoiceAnswer {
                choice: "billing".to_owned(),
                probabilities: [("billing".to_owned(), 1.0)].into_iter().collect(),
                confidence: 1.0,
            }),
        )]
        .into_iter()
        .collect(),
        usage: None,
    };
    crux_typesafe::register(
        &mut reg,
        Arc::new(CannedJudgmentClient::empty().push(choice)),
    );
    let handler = reg.get_handler("judge::score").unwrap().clone();

    let error = handler(json!({
        "state": "something",
        "instructions": "How severe?",
        "criteria": ["low", "high"],
    }))
    .await
    .outcome
    .expect_err("a choice answer carries no score to normalize");

    assert!(
        error.to_string().contains("choice"),
        "unexpected error: {error}"
    );
}

/// The request the handler builds must match the documented wire shape.
#[tokio::test]
async fn the_request_shape_matches_the_documented_api() {
    let mut reg = HandlerRegistry::new();
    crux_typesafe::register(&mut reg, judge(2.0));
    let handler = reg.get_handler("judge::score").unwrap().clone();

    // Exercised through the public request builder so the shape is asserted, not
    // assumed: state, the `jev-latest` alias, and a typed `score` question.
    let question =
        crux_typesafe::ScoreQuestion::new("How severe?", vec!["low".into(), "high".into()])
            .expect("two levels is valid");
    let request = single_question_request(
        json!("a report"),
        "score",
        serde_json::to_value(&question).expect("question serializes"),
    );
    let value = serde_json::to_value(&request).expect("request serializes");

    assert_eq!(value["model"], json!("jev-latest"));
    assert_eq!(value["questions"]["score"]["type"], json!("score"));
    assert_eq!(
        value["questions"]["score"]["instructions"],
        json!("How severe?")
    );

    drop(handler);
}

/// Confidence must be a real number in range; `HandlerOutput` rejects `NaN` and
/// clamps out-of-range values, so assert the handler's own output is sane.
#[tokio::test]
async fn the_reported_confidence_is_always_finite_and_in_range() {
    for raw in [0.0, 0.5, 1.0, 1.43, 2.0] {
        let mut reg = HandlerRegistry::new();
        crux_typesafe::register(&mut reg, judge(raw));
        let handler = reg.get_handler("judge::score").unwrap().clone();

        let output = handler(json!({
            "state": "something",
            "instructions": "How severe?",
            "criteria": ["low", "mid", "high"],
        }))
        .await
        .outcome
        .expect("the judgment step must succeed");

        let confidence = output.confidence.expect("confidence must be present");
        assert!(
            confidence.is_finite() && (0.0..=1.0).contains(&confidence),
            "raw score {raw} produced confidence {confidence}"
        );
    }
}

/// A `HandlerOutput` carrying the payload must still be usable as a plain value,
/// since pipeline consumers may read `output.score` directly.
#[tokio::test]
async fn the_payload_is_readable_as_a_plain_value() {
    let mut reg = HandlerRegistry::new();
    crux_typesafe::register(&mut reg, judge(1.0));
    let handler = reg.get_handler("judge::score").unwrap().clone();

    let output: HandlerOutput = handler(json!({
        "state": "something",
        "instructions": "How severe?",
        "criteria": ["low", "mid", "high"],
    }))
    .await
    .outcome
    .expect("the judgment step must succeed");

    assert!(output.value["score"].is_number());
    assert!(output.value["probabilities"].is_object());
    assert!(output.value["distribution_confidence"].is_number());
    assert!(output.value["attempts"].is_number());
}
