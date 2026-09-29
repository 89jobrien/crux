//! BAML-backed handlers for the analysis and confidence pipeline steps.
//!
//! Two handlers mirror the two step shapes that appear across the example
//! pipelines:
//!
//! - [`register_analyze`] installs `llm::analyze`, which turns raw evidence
//!   into a summary plus severity-tagged findings. Its result carries a
//!   confidence score, so it can drive `route_on_confidence` directly.
//! - [`register_confidence`] installs `llm::confidence`, which scores how well
//!   a given claim is supported by given evidence. Its result carries the score
//!   as the step confidence, making it the natural final stage of an
//!   `analyze` pipe.
//!
//! Both bind to the BAML `Local` client (Ollama first, then hosted providers),
//! so a local model is enough to run them without credentials. A pipeline can
//! still pin an explicit client with the `client` input field.

use crate::baml_client::async_client::B;
use baml::ClientRegistry;
use crux_runtime::prelude::CruxErr;
use crux_script::{
    ArgSchema, ArgType, ConfidenceCapability, HandlerMetadata, HandlerOutput, HandlerRegistry,
    ObjectSchema, RiskLevel, ValueSchema,
};
use serde_json::{Value, json};

const ANALYZE: &str = "llm::analyze";
const CONFIDENCE: &str = "llm::confidence";

/// Schema for a single `llm::analyze` finding.
fn finding_schema() -> ValueSchema {
    ValueSchema::object(
        ObjectSchema::new()
            .required("title", ValueSchema::String)
            .required("detail", ValueSchema::String)
            .required("severity", ValueSchema::String)
            .optional("evidence", nullable_string()),
    )
}

/// A string field that the model may legitimately return as `null`.
///
/// BAML optional (`field string?`) fields arrive as an explicit `null` rather
/// than an absent key, so `optional(..)` with a plain [`ValueSchema::String`]
/// would reject every finding whose evidence could not be located.
fn nullable_string() -> ValueSchema {
    ValueSchema::Union {
        variants: vec![ValueSchema::String, ValueSchema::Null],
    }
}

/// Argument schema for `llm::analyze`.
///
/// `input_schema` is deliberately left [`ValueSchema::Dynamic`]: these handlers
/// read every parameter from the pipeline `args` object and ignore whatever the
/// previous step produced. Declaring a typed input here would make the runner
/// validate the *upstream* value against it, which is not what these handlers
/// consume.
fn analyze_args() -> ArgSchema {
    ArgSchema::new()
        .required("subject", ArgType::String)
        .required("evidence", ArgType::String)
        .optional("focus", ArgType::String)
        .optional("client", ArgType::String)
}

/// Output contract for `llm::analyze`.
fn analyze_output_schema() -> ValueSchema {
    ValueSchema::object(
        ObjectSchema::new()
            .required("summary", ValueSchema::String)
            .required("findings", ValueSchema::array(finding_schema()))
            .required("recommendation", ValueSchema::String)
            .required("confidence", ValueSchema::Number),
    )
}

/// Argument schema for `llm::confidence`. See [`analyze_args`] on the input schema.
fn confidence_args() -> ArgSchema {
    ArgSchema::new()
        .required("claim", ArgType::String)
        .required("evidence", ArgType::String)
        .optional("criteria", ArgType::Array)
        .optional("client", ArgType::String)
}

/// Output contract for `llm::confidence`.
fn confidence_output_schema() -> ValueSchema {
    ValueSchema::object(
        ObjectSchema::new()
            .required("score", ValueSchema::Number)
            .required("level", ValueSchema::String)
            .required("reasoning", ValueSchema::String)
            .required("factors", ValueSchema::array(ValueSchema::String)),
    )
}

/// Reads a handler field, accepting either a flat key or one nested under `args`.
///
/// The built-in handlers are inconsistent about this — `llm::extract` reads flat
/// keys while `llm::plan` reads `args.*` — so both spellings are accepted here
/// rather than forcing one on pipeline authors.
fn field<'a>(input: &'a Value, key: &str) -> Option<&'a Value> {
    input
        .get(key)
        .or_else(|| input.get("args").and_then(|args| args.get(key)))
        .filter(|value| !value.is_null())
}

fn required_str(input: &Value, key: &str, handler: &str) -> Result<String, CruxErr> {
    field(input, key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CruxErr::step_failed(handler, format!("missing '{key}' field")))
}

fn optional_str(input: &Value, key: &str) -> Option<String> {
    field(input, key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn string_list(input: &Value, key: &str) -> Vec<String> {
    field(input, key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn client_override(input: &Value) -> Option<String> {
    optional_str(input, "client")
}

/// Normalises a BAML-reported score for use as a step confidence.
///
/// Step confidence is `f32`, so the score is narrowed here. `HandlerOutput::with_confidence`
/// clamps out-of-range values and rejects NaN, so a model that reports `1.4` or a
/// non-numeric value degrades rather than corrupting routing.
fn as_confidence(score: f64) -> f32 {
    if score.is_nan() {
        f32::NAN
    } else {
        score as f32
    }
}

/// Register `llm::analyze` and `llm::confidence` with the default BAML client.
pub fn register(registry: &mut HandlerRegistry) {
    register_with(registry, None);
}

/// Register `llm::analyze` and `llm::confidence`, optionally routing every call
/// through the given [`ClientRegistry`].
///
/// A `ClientRegistry` overrides the BAML `client` field on each function, which
/// is how the mock-server tests point the handlers at a canned endpoint.
pub fn register_with(registry: &mut HandlerRegistry, client_registry: Option<ClientRegistry>) {
    register_analyze_with(registry, client_registry.clone());
    register_confidence_with(registry, client_registry);
}

/// Register the `llm::analyze` handler with the default BAML client.
pub fn register_analyze(registry: &mut HandlerRegistry) {
    register_analyze_with(registry, None);
}

/// Register the `llm::analyze` handler, optionally through a [`ClientRegistry`].
pub fn register_analyze_with(
    registry: &mut HandlerRegistry,
    client_registry: Option<ClientRegistry>,
) {
    let metadata = HandlerMetadata::new(ANALYZE)
        .describe("Summarize evidence into severity-tagged findings, reporting confidence.")
        .args(analyze_args())
        .input_schema(ValueSchema::Dynamic)
        .output_schema(analyze_output_schema())
        .confidence(ConfidenceCapability::Always)
        .risk(RiskLevel::Medium)
        .deterministic(false)
        .replay_safe(false);
    registry.handler_with_metadata(metadata, move |input: Value| {
        let client_registry = client_registry.clone();
        async move {
            let subject = required_str(&input, "subject", ANALYZE)?;
            let evidence = required_str(&input, "evidence", ANALYZE)?;
            let focus = optional_str(&input, "focus");
            let client = client_override(&input);

            let call = &B.Analyze;
            let result = if let Some(ref reg) = client_registry {
                call.with_client_registry(reg)
                    .call(subject, evidence, focus)
                    .await
            } else if let Some(ref name) = client {
                call.with_client(name).call(subject, evidence, focus).await
            } else {
                call.call(subject, evidence, focus).await
            }
            .map_err(|e| CruxErr::step_failed(ANALYZE, format!("BAML error: {e}")))?;

            let findings: Vec<Value> = result
                .findings
                .iter()
                .map(|finding| {
                    json!({
                        "title": finding.title,
                        "detail": finding.detail,
                        "severity": finding.severity.to_string().to_lowercase(),
                        "evidence": finding.evidence,
                    })
                })
                .collect();

            let confidence = as_confidence(result.confidence);
            Ok(HandlerOutput::with_confidence(
                json!({
                    "summary": result.summary,
                    "findings": findings,
                    "recommendation": result.recommendation,
                    "confidence": result.confidence,
                }),
                confidence,
            ))
        }
    });
}

/// Register the `llm::confidence` handler with the default BAML client.
pub fn register_confidence(registry: &mut HandlerRegistry) {
    register_confidence_with(registry, None);
}

/// Register the `llm::confidence` handler, optionally through a [`ClientRegistry`].
pub fn register_confidence_with(
    registry: &mut HandlerRegistry,
    client_registry: Option<ClientRegistry>,
) {
    let metadata = HandlerMetadata::new(CONFIDENCE)
        .describe(
            "Score how strongly evidence supports a claim, reporting that score as confidence.",
        )
        .args(confidence_args())
        .input_schema(ValueSchema::Dynamic)
        .output_schema(confidence_output_schema())
        .confidence(ConfidenceCapability::Always)
        .risk(RiskLevel::Medium)
        .deterministic(false)
        .replay_safe(false);
    registry.handler_with_metadata(metadata, move |input: Value| {
        let client_registry = client_registry.clone();
        async move {
            let claim = required_str(&input, "claim", CONFIDENCE)?;
            let evidence = required_str(&input, "evidence", CONFIDENCE)?;
            let criteria = string_list(&input, "criteria");
            let client = client_override(&input);

            let call = &B.ScoreConfidence;
            let result = if let Some(ref reg) = client_registry {
                call.with_client_registry(reg)
                    .call(claim, evidence, &criteria)
                    .await
            } else if let Some(ref name) = client {
                call.with_client(name)
                    .call(claim, evidence, &criteria)
                    .await
            } else {
                call.call(claim, evidence, &criteria).await
            }
            .map_err(|e| CruxErr::step_failed(CONFIDENCE, format!("BAML error: {e}")))?;

            let confidence = as_confidence(result.score);
            Ok(HandlerOutput::with_confidence(
                json!({
                    "score": result.score,
                    "level": result.level.to_string().to_lowercase(),
                    "reasoning": result.reasoning,
                    "factors": result.factors,
                }),
                confidence,
            ))
        }
    });
}
