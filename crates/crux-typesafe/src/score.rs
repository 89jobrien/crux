//! The `judge::score` handler.
//!
//! Asks a TypeSafe System One model to rate some state against an ordered
//! rubric, then converts the answer into a calibrated confidence that
//! `route_on_confidence` — including from YAML — can route on.

use std::sync::Arc;

use crux_runtime::prelude::CruxErr;
use crux_script::{
    ArgSchema, ConfidenceCapability, HandlerMetadata, HandlerOutput, HandlerRegistry, ObjectSchema,
    RiskLevel, SideEffect, ValueSchema,
};
use serde_json::{Value, json};

use crate::calibration::{ScoreEvidence, calibrate_score};
use crate::client::{JudgmentClient, take_answer};
use crate::wire::{Answer, ScoreAnswer, ScoreQuestion};

/// Handler name, matching the `ns::verb` convention used across the registry.
pub const SCORE: &str = "judge::score";

/// Question id used when the caller does not name one.
const DEFAULT_QUESTION_ID: &str = "score";

/// Backend calls the handler is able to observe.
///
/// FIXME(typesafe-followup): pinned at 1 because the judgment port does not carry
/// a retry count, which makes `RETRY_DECAY` unreachable in production — a
/// confident routing value and a flapping backend are currently indistinguishable
/// (finding #3).
///
/// The attempt count is a property of the *transport*, not of the response body:
/// `HttpJudgmentClient::evaluate` counts attempts in a local loop (client.rs,
/// `attempt`/`delay_for`) and returns only the winning `SystemOneResponse`.
/// Nothing in `SystemOneResponse` encodes how many calls produced it — `usage`
/// holds token counts, and a retried call's failures never return tokens — so no
/// correct fix exists inside this file. The required change, in files this fix
/// does not own:
///
/// 1. `wire.rs` — add `attempts: u32` to `SystemOneResponse`, `#[serde(default)]`
///    with `#[serde(skip)]` semantics for decoding (the API does not send it).
/// 2. `client.rs` — in `JudgmentClient for HttpJudgmentClient::evaluate`, set
///    `response.attempts = attempt` on the response it returns.
/// 3. `score.rs` — delete this constant and pass `response.attempts` to
///    `ScoreEvidence::new`.
///
/// Steps 1 and 2 belong to the client/wire owners. Until they land,
/// `the_handler_reports_one_attempt_while_the_port_hides_retries` pins the
/// behaviour so this cannot be quietly forgotten.
const OBSERVED_ATTEMPTS: u32 = 1;

/// Register `judge::score` against the given judgment backend.
///
/// Every call is routed through `client`. Passing a
/// [`crate::client::CannedJudgmentClient`] here is what lets the regression suite
/// exercise routing without an API key.
///
/// This is the only registration entry point. `register_score` used to be a
/// byte-for-byte duplicate of it (finding #18) and survives only because `lib.rs`
/// still re-exports the name.
pub fn register(registry: &mut HandlerRegistry, client: Arc<dyn JudgmentClient>) {
    let metadata = metadata();
    registry.handler_with_metadata(metadata, move |input: Value| {
        let client = Arc::clone(&client);
        async move { run(&*client, input).await }
    });
}

/// Duplicate of [`register`], kept only so `lib.rs` keeps compiling.
///
/// FIXME(typesafe-followup): delete this function and drop `register_score` from
/// `pub use score::{register, register_score};` in `lib.rs`, which this fix does
/// not own. That re-export is the only remaining caller (finding #18); no test or
/// other crate references the name.
pub fn register_score(registry: &mut HandlerRegistry, client: Arc<dyn JudgmentClient>) {
    register(registry, client);
}

fn metadata() -> HandlerMetadata {
    HandlerMetadata::new(SCORE)
        .describe(
            "Rate state against an ordered rubric with a calibrated System One judgment; \
             reports step confidence so route_on_confidence can use it.",
        )
        .args(
            ArgSchema::new()
                .required("state", ValueSchema::Dynamic)
                .required("instructions", ValueSchema::String)
                .required(
                    "criteria",
                    ValueSchema::Array {
                        items: Box::new(ValueSchema::String),
                    },
                )
                .optional("question_id", ValueSchema::String),
        )
        .input_schema(ValueSchema::Dynamic)
        .output_schema(output_schema())
        // Every successful call reports a calibrated confidence, which is what
        // lets a YAML pipeline route on `{{ steps.<name>.confidence }}` at all.
        // Declaring `Always` also stops `crux run --check` raising the
        // `dynamic_boundary` diagnostic for this step.
        .confidence(ConfidenceCapability::Always)
        .risk(RiskLevel::Medium)
        .side_effects(vec![SideEffect::Network, SideEffect::Llm])
        .deterministic(false)
        .replay_safe(false)
}

fn output_schema() -> ValueSchema {
    ValueSchema::Object(
        ObjectSchema::new()
            .required("score", ValueSchema::Number)
            .required("normalized", ValueSchema::Number)
            .required(
                "probabilities",
                ValueSchema::Object(ObjectSchema::new().additional(ValueSchema::Number)),
            )
            .required("distribution_confidence", ValueSchema::Number)
            .required("attempts", ValueSchema::Integer)
            .required("retry_multiplier", ValueSchema::Number),
    )
}

/// Parse and validate a handler input payload.
struct ScoreRequest {
    question_id: String,
    question: ScoreQuestion,
    state: Value,
}

fn parse_request(input: &Value) -> Result<ScoreRequest, String> {
    // The pipeline runner nests a step's `args:` under an `args` key; a direct
    // call passes the fields at the top level. Accept either so the handler works
    // from a `.crux` file and from a Rust caller without a translation layer.
    let args = input.get("args").unwrap_or(input);

    let state = args
        .get("state")
        .cloned()
        .ok_or_else(|| "missing 'state' field".to_owned())?;

    let instructions = args
        .get("instructions")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing or non-string 'instructions' field".to_owned())?
        .to_owned();

    let criteria: Vec<String> = args
        .get("criteria")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing or non-array 'criteria' field".to_owned())?
        .iter()
        .map(|level| {
            level
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| "every 'criteria' entry must be a string".to_owned())
        })
        .collect::<Result<_, _>>()?;

    let question = ScoreQuestion::new(instructions, criteria).map_err(|error| error.to_string())?;

    Ok(ScoreRequest {
        question_id: args
            .get("question_id")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_QUESTION_ID)
            .to_owned(),
        question,
        state,
    })
}

async fn run(client: &dyn JudgmentClient, input: Value) -> Result<HandlerOutput, CruxErr> {
    let request = parse_request(&input).map_err(|message| CruxErr::step_failed(SCORE, message))?;

    let response = client
        .evaluate(crate::client::single_question_request(
            request.state,
            &request.question_id,
            serde_json::to_value(&request.question)
                .map_err(|error| CruxErr::step_failed(SCORE, error.to_string()))?,
        ))
        .await
        .map_err(|error| CruxErr::step_failed(SCORE, error.to_string()))?;

    let answer = take_answer(&response, &request.question_id)
        .map_err(|error| CruxErr::step_failed(SCORE, error.to_string()))?;

    let ScoreAnswer {
        score,
        legend,
        probabilities,
        confidence: distribution_confidence,
        ..
    } = match answer {
        Answer::Score(answer) => answer.clone(),
        // A choice or noul answer has no rubric position, so there is nothing to
        // normalize. Failing loudly beats routing on an unrelated number.
        other => {
            return Err(CruxErr::step_failed(
                SCORE,
                format!(
                    "expected a score answer for question '{}', got {:?}",
                    request.question_id,
                    other_kind(other)
                ),
            ));
        }
    };

    let evidence = ScoreEvidence::new(
        ScoreAnswer {
            score,
            legend,
            probabilities,
            confidence: distribution_confidence,
        },
        // The request's `criteria` is the authority on how many levels the rubric
        // has, not the legend the backend echoed back (finding #8).
        request.question.criteria.len(),
        OBSERVED_ATTEMPTS,
    );

    // `CruxErr::step_failed` already prefixes the step name, so the calibration
    // error is rendered on its own. Prefixing it here too produced
    // "step 'judge::score' failed: judge::score: ..." (finding #9).
    let (payload, confidence) = calibrate_score(&evidence)
        .map_err(|error| CruxErr::step_failed(SCORE, error.to_string()))?;

    Ok(HandlerOutput::with_confidence(payload, confidence))
}

fn other_kind(answer: &Answer) -> &'static str {
    match answer {
        Answer::Score(_) => "score",
        Answer::Choice(_) => "choice",
        Answer::Noul(_) => "noul",
    }
}

/// Build the payload a caller would send, for documentation and tests.
///
/// # Example
///
/// The rubric width in `criteria` is what the handler normalizes against, so a
/// caller changing the number of levels changes the mapping. This walks the full
/// calibration for a top-of-rubric answer:
///
/// ```
/// use crux_typesafe::calibration::{ScoreEvidence, calibrate_score};
/// use crux_typesafe::score::example_input;
/// use crux_typesafe::wire::ScoreAnswer;
/// use serde_json::Value;
/// use std::collections::BTreeMap;
///
/// let input = example_input();
/// let levels = input["criteria"].as_array().expect("criteria is an array").len();
///
/// let legend = (0..levels)
///     .map(|level| (level.to_string(), Value::from(format!("level {level}"))))
///     .collect();
///
/// let evidence = ScoreEvidence::new(
///     ScoreAnswer {
///         score: 2.0,
///         legend,
///         probabilities: BTreeMap::from([("2".to_owned(), 1.0)]),
///         confidence: 0.9,
///     },
///     levels,
///     1,
/// );
///
/// let (payload, confidence) = calibrate_score(&evidence).expect("a 3-level rubric calibrates");
///
/// assert_eq!(payload["score"], serde_json::json!(2.0));
/// assert_eq!(payload["normalized"], serde_json::json!(1.0));
/// assert_eq!(payload["distribution_confidence"], serde_json::json!(0.9));
/// assert_eq!(confidence, 1.0);
/// ```
pub fn example_input() -> Value {
    json!({
        "state": "The export button crashes the settings page in Safari.",
        "instructions": "How severe is the reported issue?",
        "criteria": [
            "Cosmetic; no impact to functionality",
            "Broken or degraded feature, but workaround exists",
            "Blocking issue; no workaround exists",
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use crate::client::CannedJudgmentClient;
    use crate::wire::SystemOneResponse;

    /// A canned response scoring `score`, echoing a `legend_width`-entry legend.
    ///
    /// `legend_width` is deliberately decoupled from the request's criteria count
    /// so a test can simulate a backend whose echoed legend disagrees with what
    /// the caller actually sent.
    fn score_response(score: f64, legend_width: usize) -> SystemOneResponse {
        let legend = (0..legend_width)
            .map(|level| (level.to_string(), json!("level {level}")))
            .collect();
        SystemOneResponse {
            model: "jev-test".to_owned(),
            answers: [(
                DEFAULT_QUESTION_ID.to_owned(),
                Answer::Score(ScoreAnswer {
                    score,
                    legend,
                    probabilities: BTreeMap::from([("0".to_owned(), 1.0)]),
                    confidence: 0.5,
                }),
            )]
            .into_iter()
            .collect(),
            usage: None,
        }
    }

    fn judge(score: f64, legend_width: usize) -> CannedJudgmentClient {
        CannedJudgmentClient::empty().push(score_response(score, legend_width))
    }

    /// Drive the handler over `example_input()`, whose rubric has three levels.
    async fn judge_example(score: f64, legend_width: usize) -> Result<HandlerOutput, CruxErr> {
        run(&judge(score, legend_width), example_input()).await
    }

    /// Finding #9: `CruxErr::step_failed` already prefixes the step name, so an
    /// inner prefix rendered it twice — "step 'judge::score' failed: judge::score: ...".
    #[tokio::test]
    async fn a_calibration_failure_names_the_step_exactly_once() {
        let error = judge_example(f64::NAN, 3)
            .await
            .expect_err("a non-finite score cannot be calibrated");
        let rendered = error.to_string();

        assert_eq!(
            rendered.matches(SCORE).count(),
            1,
            "the step name must appear exactly once, got {rendered:?}"
        );
        assert!(
            rendered.starts_with(&format!("step '{SCORE}' failed: ")),
            "unexpected message shape: {rendered:?}"
        );
        assert!(
            rendered.contains("non-finite"),
            "the underlying cause must survive: {rendered:?}"
        );
    }

    /// Finding #8: the request's `criteria.len()` is the authoritative width. A
    /// backend echoing a wider `legend` must not rescale the same judgment.
    #[tokio::test]
    async fn a_mismatched_echoed_legend_cannot_change_the_routing_value() {
        // example_input() asks for 3 levels, so the top level number is 2. A score
        // of 1.0 is the midpoint, hence 0.5 — no matter what the legend claims.
        let output = judge_example(1.0, 3)
            .await
            .expect("the judgment step must succeed");
        let honest = output.confidence.expect("confidence must be present");

        // Same score, same request, but the backend echoes a 9-entry legend.
        let output = judge_example(1.0, 9)
            .await
            .expect("the judgment step must succeed");
        let tampered = output.confidence.expect("confidence must be present");

        assert!(
            (honest - 0.5).abs() < 1e-6,
            "1.0 of 0..=2 must normalize to 0.5, got {honest}"
        );
        assert_eq!(
            tampered, honest,
            "a wider echoed legend must not rescale the judgment"
        );
    }

    /// Finding #3: the handler cannot yet observe a retry count, so `RETRY_DECAY`
    /// never fires in production. This pins that gap so it cannot be forgotten —
    /// see the `FIXME(typesafe-followup)` on [`OBSERVED_ATTEMPTS`].
    #[tokio::test]
    async fn the_handler_reports_one_attempt_while_the_port_hides_retries() {
        let output = judge_example(2.0, 3)
            .await
            .expect("the judgment step must succeed");

        assert_eq!(output.value["attempts"], json!(1));
        assert_eq!(
            output.value["retry_multiplier"],
            json!(1.0),
            "RETRY_DECAY cannot apply until the port surfaces an attempt count"
        );
    }

    /// Finding #14: `output_schema()` and `JudgmentPayload` are written by hand
    /// on either side of a module boundary. `validate` rejects an unknown key on a
    /// closed object and a missing required one, so this catches drift in both
    /// directions rather than only the direction the reviewer happened to read.
    #[tokio::test]
    async fn the_output_schema_exactly_describes_the_payload() {
        let output = judge_example(1.43, 3)
            .await
            .expect("the judgment step must succeed");

        output_schema()
            .validate(&output.value)
            .unwrap_or_else(|violation| {
                panic!("the declared output schema must accept the real payload: {violation}")
            });
    }
}
