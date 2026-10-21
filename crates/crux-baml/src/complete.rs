//! BAML-backed implementations of the generic LLM pipeline handlers.
//!
//! `llm::invoke`, `llm::invoke_with_fallback`, and `llm::stream` all route
//! through a single BAML function ([`Complete`]) so provider selection, retries,
//! and output parsing live in one place. `llm::invoke_with_fallback` is a thin
//! wrapper over `llm::invoke` that pins the BAML fallback client, since BAML
//! already implements the tiered failover the hand-rolled loop duplicated.
//!
//! # Structured output
//!
//! [`Complete`] returns a BAML class marked `@@dynamic`, so a pipeline can ask
//! for any output shape it likes by passing a JSON Schema:
//!
//! ```yaml
//! - step: review
//!   handler: llm::invoke
//!   args:
//!     prompt: "Review this diff: {{ steps.diff.output.stdout }}"
//!     schema:
//!       type: object
//!       properties:
//!         verdict: { type: string, enum: [pass, block, comment] }
//!         blockers: { type: array, items: { type: string } }
//!         score: { type: number }
//! ```
//!
//! Omit `schema` and the call stays free-text — the dynamic class carries only
//! its declared `content` field. Injected fields are merged alongside `content`
//! in the handler output, so templates read `{{ steps.review.output.verdict }}`.
//!
//! [`Complete`]: crate::baml_client::async_client::B::Complete

use crate::baml_client::async_client::B;
use crate::confidence::CallEvidence;
use baml::{ClientRegistry, Collector};
use crux_runtime::prelude::CruxErr;
use crux_script::{
    ArgSchema, ArgType, ConfidenceCapability, HandlerMetadata, HandlerOutput, HandlerRegistry,
    ObjectSchema, RiskLevel, ValueSchema,
};
use serde_json::{Value, json};

const INVOKE: &str = "llm::invoke";
const FALLBACK: &str = "llm::invoke_with_fallback";
const STREAM: &str = "llm::stream";

/// BAML client used when a pipeline does not pin one: local Ollama first.
const DEFAULT_CLIENT: &str = "Local";
/// BAML client used by `llm::invoke_with_fallback`, which walks providers in order.
const FALLBACK_CLIENT: &str = "Auto";

const DEFAULT_MAX_TOKENS: i64 = 1024;

/// Arguments shared by all three handlers.
fn common_args() -> ArgSchema {
    ArgSchema::new()
        .required("prompt", ArgType::String)
        .optional("system", ArgType::String)
        .optional("max_tokens", ArgType::Integer)
        .optional("schema", ArgType::Object)
        .optional("client", ArgType::String)
}

fn output_schema(streaming: bool) -> ValueSchema {
    let mut schema = ObjectSchema::new()
        .required("content", ValueSchema::String)
        .required("client", ValueSchema::String)
        .required("schema_valid", ValueSchema::Boolean)
        .required("confidence", ValueSchema::Number)
        .required("attempts", ValueSchema::Number);
    if streaming {
        schema = schema
            .required("streaming", ValueSchema::Boolean)
            .required("chunks", ValueSchema::array(ValueSchema::String));
    }
    // Injected fields land here as arbitrary JSON. `additional` keeps the schema
    // open so a pipeline's own `schema` output validates without the compiler
    // having to enumerate every property.
    ValueSchema::object(schema.additional(ValueSchema::Dynamic))
}

/// A parsed `llm::invoke`-style request.
struct CompletionRequest {
    prompt: String,
    system: Option<String>,
    max_tokens: i64,
    schema: Option<Value>,
    client: Option<String>,
}

/// Reads a handler field, accepting either a flat key or one nested under `args`.
///
/// The built-in handlers are inconsistent about this — `llm::extract` reads flat
/// keys while `llm::plan` reads `args.*` — so both spellings are accepted rather
/// than forcing one on pipeline authors.
fn field<'a>(input: &'a Value, key: &str) -> Option<&'a Value> {
    input
        .get(key)
        .or_else(|| input.get("args").and_then(|args| args.get(key)))
        .filter(|value| !value.is_null())
}

fn parse_request(input: &Value, handler: &str) -> Result<CompletionRequest, CruxErr> {
    let prompt = field(input, "prompt")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CruxErr::step_failed(handler, "missing 'prompt' field"))?;

    Ok(CompletionRequest {
        prompt,
        system: field(input, "system")
            .and_then(Value::as_str)
            .map(str::to_string),
        max_tokens: field(input, "max_tokens")
            .and_then(Value::as_u64)
            .map(|value| value as i64)
            .unwrap_or(DEFAULT_MAX_TOKENS),
        schema: field(input, "schema").cloned(),
        client: field(input, "client")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// Significant digits kept in the payload's `confidence`.
///
/// Step confidence is `f32`, so widening it to `f64` for JSON exposes the binary
/// representation (`0.8` becomes `0.800000011920929`). Rounding to 6 places is
/// well inside `f32` precision — the value still compares equal to the step
/// confidence — while keeping traces and CLI output readable.
const PAYLOAD_CONFIDENCE_PLACES: i32 = 6;

/// A flattened completion, ready to be merged into handler output.
struct Completion {
    content: String,
    fields: Value,
    evidence: CallEvidence,
}

impl Completion {
    /// Merge into the handler output, then stamp the BAML provenance fields.
    ///
    /// `confidence` is also written into the payload so templates can read
    /// `{{ steps.x.output.confidence }}`; the same value is attached to the
    /// `HandlerOutput` by [`finish`].
    fn into_output(self, client: &str, chunks: Option<Vec<String>>) -> Value {
        let confidence = self.evidence.confidence();
        let mut out = match self.fields {
            Value::Object(map) => map,
            other => {
                let mut map = serde_json::Map::new();
                map.insert("fields".to_string(), other);
                map
            }
        };
        out.insert("content".to_string(), Value::String(self.content));
        out.insert("client".to_string(), Value::String(client.to_string()));
        out.insert("schema_valid".to_string(), Value::Bool(true));
        out.insert(
            "confidence".to_string(),
            json!(round_to(confidence, PAYLOAD_CONFIDENCE_PLACES)),
        );
        out.insert("attempts".to_string(), json!(self.evidence.attempts));
        if let Some(chunks) = chunks {
            out.insert("streaming".to_string(), Value::Bool(true));
            out.insert(
                "chunks".to_string(),
                Value::Array(chunks.into_iter().map(Value::String).collect()),
            );
        }
        Value::Object(out)
    }
}

/// Round to `places` decimal places, leaving non-finite values untouched.
fn round_to(value: f32, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    let scaled = f64::from(value) * scale;
    if !scaled.is_finite() {
        return f64::from(value);
    }
    (scaled.round() / scale).clamp(0.0, 1.0)
}

/// Attach a completion's payload and its confidence to a [`HandlerOutput`].
///
/// The score is computed once here and written into both the payload and the step
/// confidence, so a template reading `output.confidence` and a
/// `route_on_confidence` on `steps.<name>.confidence` always agree.
fn finish(
    completion: Completion,
    client: &str,
    chunks: Option<Vec<String>>,
) -> Result<HandlerOutput, CruxErr> {
    let confidence = completion.evidence.confidence();
    Ok(HandlerOutput::with_confidence(
        completion.into_output(client, chunks),
        confidence,
    ))
}

/// Run one completion against BAML, applying the type builder and client override.
async fn run(
    handler: &str,
    request: &CompletionRequest,
    client_registry: Option<&ClientRegistry>,
    default_client: &str,
) -> Result<Completion, CruxErr> {
    // `None` means free text: the dynamic class carries only `content` and
    // `confidence`.
    let type_builder = match request.schema.as_ref() {
        None => None,
        Some(schema) => crate::schema_bridge::type_builder_for(schema)
            .map_err(|error| CruxErr::step_failed(handler, error.to_string()))?,
    };

    // A collector records every call BAML made, including parse retries, which is
    // the objective half of the confidence score.
    let collector = crate::baml_client::new_collector("crux-completion");

    let call = &B.Complete;
    let pinned = request.client.as_deref().unwrap_or(default_client);
    let call = call.with_collector(&collector);
    let call = match &type_builder {
        Some(builder) => call.with_type_builder(builder),
        None => call,
    };
    let call = match client_registry {
        Some(registry) => call.with_client_registry(registry),
        None => call.with_client(pinned),
    };

    let result = call
        .call(
            request.prompt.clone(),
            request.system.clone(),
            Some(request.max_tokens),
        )
        .await
        .map_err(|error| CruxErr::step_failed(handler, format!("BAML error: {error}")))?;

    let attempts = attempt_count(&collector);
    let evidence = CallEvidence::new(result.confidence, attempts);
    let fields = fields_to_json(&result);
    Ok(Completion {
        content: result.content,
        fields,
        evidence,
    })
}

/// How many times BAML called the model, including parse retries.
///
/// `FunctionLog::calls()` returns "all calls made (including retries)", so a
/// count above one means earlier answers failed validation. A collector that
/// recorded nothing (which should not happen after a successful call) is treated
/// as a single attempt rather than as maximum distrust.
fn attempt_count(collector: &Collector) -> u32 {
    collector
        .logs()
        .iter()
        .map(|log| log.calls().len() as u32)
        .max()
        .unwrap_or(1)
}

/// Flatten the `@@dynamic` fields BAML parsed alongside `content`.
fn fields_to_json(completion: &crate::baml_client::types::Completion) -> Value {
    let mut map = serde_json::Map::new();
    for (name, value) in &completion.__dynamic {
        map.insert(name.clone(), baml_value_to_json(value));
    }
    Value::Object(map)
}

/// Convert a BAML dynamic value into plain JSON.
///
/// Uses `BamlValue`'s `Serialize` impl rather than `BamlValue::get`, which only
/// accepts the fixed set of BAML types and rejects `serde_json::Value`. A field
/// the model failed to populate becomes `null` rather than disappearing, so a
/// template reading it sees "not answered" instead of a missing-path error.
fn baml_value_to_json(value: &crate::baml_client::BamlValue) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// `ConfidenceCapability::Always` because every completion carries a score: the
/// model's self-report discounted by how many attempts BAML needed to reach a
/// parseable answer (see [`crate::confidence`]). Declaring it means any completion
/// step can feed `route_on_confidence` directly.
fn metadata(name: &str, description: &str, args: ArgSchema, streaming: bool) -> HandlerMetadata {
    HandlerMetadata::new(name)
        .describe(description)
        .args(args)
        .input_schema(ValueSchema::Dynamic)
        .output_schema(output_schema(streaming))
        .confidence(ConfidenceCapability::Always)
        .risk(RiskLevel::Medium)
        .deterministic(false)
        .replay_safe(false)
}

/// Register `llm::invoke`, `llm::invoke_with_fallback`, and `llm::stream`.
pub fn register(registry: &mut HandlerRegistry) {
    register_with(registry, None);
}

/// Register the three handlers, optionally through a [`ClientRegistry`].
pub fn register_with(registry: &mut HandlerRegistry, client_registry: Option<ClientRegistry>) {
    register_invoke_with(registry, client_registry.clone());
    register_fallback_with(registry, client_registry.clone());
    register_stream_with(registry, client_registry);
}

/// Register the `llm::invoke` handler.
pub fn register_invoke(registry: &mut HandlerRegistry) {
    register_invoke_with(registry, None);
}

/// Register the `llm::invoke` handler, optionally through a [`ClientRegistry`].
pub fn register_invoke_with(
    registry: &mut HandlerRegistry,
    client_registry: Option<ClientRegistry>,
) {
    let metadata = metadata(
        INVOKE,
        "LLM completion routed through BAML; pass 'schema' for structured output.",
        common_args(),
        false,
    );
    registry.handler_with_metadata(metadata, move |input: Value| {
        let client_registry = client_registry.clone();
        async move {
            let request = parse_request(&input, INVOKE)?;
            let client = request.client.as_deref().unwrap_or(DEFAULT_CLIENT);
            let completion =
                run(INVOKE, &request, client_registry.as_ref(), DEFAULT_CLIENT).await?;
            finish(completion, client, None)
        }
    });
}

/// Register the `llm::invoke_with_fallback` handler.
///
/// The historical `tiers` argument is accepted but no longer consulted: BAML's
/// fallback client already walks providers in order, so the handler pins it and
/// lets BAML decide. BAML's own guidance is that a fallback client beats a
/// hand-written tier loop, which is what this handler used to be.
pub fn register_fallback(registry: &mut HandlerRegistry) {
    register_fallback_with(registry, None);
}

/// Register the `llm::invoke_with_fallback` handler, optionally through a [`ClientRegistry`].
pub fn register_fallback_with(
    registry: &mut HandlerRegistry,
    client_registry: Option<ClientRegistry>,
) {
    let metadata = metadata(
        FALLBACK,
        "LLM completion with provider failover, routed through the BAML fallback client.",
        common_args().optional("tiers", ArgType::Array),
        false,
    );
    registry.handler_with_metadata(metadata, move |input: Value| {
        let client_registry = client_registry.clone();
        async move {
            let request = parse_request(&input, FALLBACK)?;
            let client = request
                .client
                .as_deref()
                .unwrap_or(FALLBACK_CLIENT)
                .to_string();
            let completion = run(
                FALLBACK,
                &request,
                client_registry.as_ref(),
                FALLBACK_CLIENT,
            )
            .await?;
            finish(completion, &client, None)
        }
    });
}

/// Register the `llm::stream` handler.
pub fn register_stream(registry: &mut HandlerRegistry) {
    register_stream_with(registry, None);
}

/// Register the `llm::stream` handler, optionally through a [`ClientRegistry`].
///
/// BAML's own docs note that a plain `.call()` is faster and more efficient when
/// the consumer only needs the final value, so prefer `llm::invoke` for that.
/// `llm::stream` stays available for pipelines that read `chunks`; because the
/// `Completion` type is validated as a whole by BAML, the assembled content is
/// returned as a single chunk.
pub fn register_stream_with(
    registry: &mut HandlerRegistry,
    client_registry: Option<ClientRegistry>,
) {
    let metadata = metadata(
        STREAM,
        "Streaming LLM completion routed through BAML; returns assembled content and chunks.",
        common_args(),
        true,
    );
    registry.handler_with_metadata(metadata, move |input: Value| {
        let client_registry = client_registry.clone();
        async move {
            let request = parse_request(&input, STREAM)?;
            let client = request.client.as_deref().unwrap_or(DEFAULT_CLIENT);
            let completion =
                run(STREAM, &request, client_registry.as_ref(), DEFAULT_CLIENT).await?;
            let chunks = vec![completion.content.clone()];
            finish(completion, client, Some(chunks))
        }
    });
}

/// Re-exported so callers can build a type builder without depending on the
/// generated client directly.
pub use crate::baml_client::type_builder::TypeBuilder as SchemaTypeBuilder;

#[cfg(test)]
mod tests {
    use super::*;

    /// The payload value must survive the f32 -> f64 widening without exposing
    /// the binary representation, while still comparing equal to the step
    /// confidence a route sees.
    #[test]
    fn payload_confidence_is_round_trippable() {
        for raw in [0.0f32, 0.2, 0.5, 0.8, 0.86, 1.0] {
            let rounded = round_to(raw, PAYLOAD_CONFIDENCE_PLACES);
            assert!(
                (rounded - f64::from(raw)).abs() < 1e-6,
                "rounding {raw} to 6 places moved it to {rounded}"
            );
        }
    }

    #[test]
    fn payload_confidence_has_no_float_noise() {
        // 0.8f32 widens to 0.80000001192092896; rounding must not leak that.
        assert_eq!(round_to(0.8, PAYLOAD_CONFIDENCE_PLACES), 0.8);
        assert_eq!(round_to(0.86, PAYLOAD_CONFIDENCE_PLACES), 0.86);
    }

    #[test]
    fn payload_confidence_stays_in_range() {
        for raw in [0.0f32, 0.5, 1.0, f32::MIN_POSITIVE, 0.999_999_9] {
            let rounded = round_to(raw, PAYLOAD_CONFIDENCE_PLACES);
            assert!((0.0..=1.0).contains(&rounded), "{raw} -> {rounded}");
        }
    }
}
