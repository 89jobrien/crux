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

/// Register `judge::score` against the default judgment backend.
pub fn register(registry: &mut HandlerRegistry, client: Arc<dyn JudgmentClient>) {
    register_score(registry, client);
}

/// Register `judge::score`, routing every call through `client`.
///
/// Passing a [`crate::client::CannedJudgmentClient`] here is what lets the
/// regression suite exercise routing without an API key.
pub fn register_score(registry: &mut HandlerRegistry, client: Arc<dyn JudgmentClient>) {
    let metadata = metadata();
    registry.handler_with_metadata(metadata, move |input: Value| {
        let client = Arc::clone(&client);
        async move { run(&*client, input).await }
    });
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
        1,
    );

    let (payload, confidence) = calibrate_score(&evidence)
        .map_err(|error| CruxErr::step_failed(SCORE, error.step_failed(SCORE)))?;

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
