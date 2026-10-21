//! Mock-server integration tests for the BAML-routed generic LLM handlers:
//! `llm::invoke`, `llm::invoke_with_fallback`, and `llm::stream`.
//!
//! These prove the handlers reach BAML and that a caller-supplied JSON Schema is
//! translated into injected BAML output fields. No API keys required.

mod mock_baml;

use crate::mock_baml::{MockBamlServer, default_responses};
use crux_baml::complete;
use crux_script::{HandlerOutput, HandlerRegistry};
use serde_json::{Value, json};

async fn make_mock_registry() -> (MockBamlServer, HandlerRegistry) {
    let server = MockBamlServer::start(default_responses()).await;
    let client_registry = server.registry();
    let mut registry = HandlerRegistry::new();
    complete::register_with(&mut registry, Some(client_registry));
    (server, registry)
}

async fn invoke(
    registry: &HandlerRegistry,
    handler: &str,
    input: Value,
) -> Result<HandlerOutput, Box<dyn std::error::Error + Send + Sync>> {
    let handler = registry
        .get_handler(handler)
        .unwrap_or_else(|| panic!("{handler} handler must be registered"));
    Ok(handler(input).await.outcome?)
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "risk_level": { "type": "string", "enum": ["low", "medium", "high"] },
            "blockers": { "type": "array", "items": { "type": "string" } },
            "score": { "type": "number" }
        }
    })
}

// -- free text ---------------------------------------------------------------

#[tokio::test]
async fn invoke_returns_free_text_without_a_schema() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({ "prompt": "free-text-completion-prompt" });

    let result = invoke(&registry, "llm::invoke", input)
        .await
        .expect("llm::invoke should succeed");

    assert_eq!(
        result.get("content").and_then(Value::as_str),
        Some("Paris is the capital of France.")
    );
    assert_eq!(result.get("schema_valid"), Some(&Value::Bool(true)));
    assert_eq!(result.get("client").and_then(Value::as_str), Some("Local"));
}

// -- confidence --------------------------------------------------------------

/// Every LLM call must be able to feed `route_on_confidence`, so even a plain
/// free-text completion carries a step confidence.
#[tokio::test]
async fn every_completion_reports_a_step_confidence() {
    for handler in ["llm::invoke", "llm::stream", "llm::invoke_with_fallback"] {
        let (_server, registry) = make_mock_registry().await;
        let input = json!({ "prompt": "free-text-completion-prompt" });

        let result = invoke(&registry, handler, input)
            .await
            .unwrap_or_else(|error| panic!("{handler} should succeed: {error}"));

        let confidence = result
            .confidence
            .unwrap_or_else(|| panic!("{handler} must report a confidence score"));
        assert!(
            (0.0..=1.0).contains(&confidence),
            "{handler} confidence must be in [0.0, 1.0], got {confidence}"
        );
    }
}

/// The payload's `confidence` and the step confidence must be the same number,
/// otherwise a template and a route would disagree about how sure the step was.
#[tokio::test]
async fn payload_confidence_matches_step_confidence() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({ "prompt": "free-text-completion-prompt" });

    let result = invoke(&registry, "llm::invoke", input)
        .await
        .expect("llm::invoke should succeed");

    let payload = result
        .get("confidence")
        .and_then(Value::as_f64)
        .expect("payload must carry 'confidence'");
    let step = result
        .confidence
        .expect("step must report a confidence score");
    assert!((payload - f64::from(step)).abs() < 1e-6);
}

/// The attempt count is what the retry penalty is derived from, so it must be
/// reported alongside the score.
#[tokio::test]
async fn completion_reports_how_many_attempts_baml_made() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({ "prompt": "free-text-completion-prompt" });

    let result = invoke(&registry, "llm::invoke", input)
        .await
        .expect("llm::invoke should succeed");

    let attempts = result
        .get("attempts")
        .and_then(Value::as_u64)
        .expect("payload must carry 'attempts'");
    assert!(
        attempts >= 1,
        "a successful call is at least one attempt, got {attempts}"
    );
}

/// A schema may not redefine `confidence`: the handler lifts the value out of
/// the BAML class, so an injected field of the same name would desync the payload
/// from the step confidence.
#[tokio::test]
async fn schema_may_not_redefine_confidence() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "prompt": "review this diff",
        "schema": {
            "type": "object",
            "properties": { "confidence": { "type": "number" } }
        }
    });

    let err = invoke(&registry, "llm::invoke", input)
        .await
        .expect_err("redefining 'confidence' must be rejected");
    let message = err.to_string();
    assert!(
        message.contains("confidence") && message.contains("already"),
        "error should explain the conflict, got: {message}"
    );
}

#[tokio::test]
async fn schema_may_not_redefine_content() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "prompt": "review this diff",
        "schema": {
            "type": "object",
            "properties": { "content": { "type": "string" } }
        }
    });

    let err = invoke(&registry, "llm::invoke", input)
        .await
        .expect_err("redefining 'content' must be rejected");
    assert!(
        err.to_string().contains("content"),
        "error should name the field, got: {err}"
    );
}

// -- structured output -------------------------------------------------------

/// The whole point of routing through BAML: a caller-supplied JSON Schema
/// becomes validated output fields on an otherwise free-text handler.
#[tokio::test]
async fn invoke_injects_caller_supplied_schema_fields() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({ "prompt": "review this diff", "schema": schema() });

    let result = invoke(&registry, "llm::invoke", input)
        .await
        .expect("llm::invoke should succeed");

    assert_eq!(
        result.get("risk_level").and_then(Value::as_str),
        Some("high"),
        "injected enum field must come back"
    );
    let blockers = result
        .get("blockers")
        .and_then(Value::as_array)
        .expect("injected array field must come back");
    assert_eq!(blockers.len(), 1);
    assert_eq!(
        result.get("score").and_then(Value::as_f64),
        Some(0.2),
        "injected number field must come back"
    );
    // `content` survives alongside the injected fields.
    assert_eq!(
        result.get("content").and_then(Value::as_str),
        Some("Found one blocking issue in the auth path.")
    );
}

/// A bare `{field: {schema}}` map is accepted as shorthand for a full JSON Schema.
#[tokio::test]
async fn invoke_accepts_a_bare_field_map() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "prompt": "review this diff",
        "schema": {
            "risk_level": { "type": "string", "enum": ["low", "medium", "high"] },
            "blockers": { "type": "array", "items": { "type": "string" } },
            "score": { "type": "number" }
        }
    });

    let result = invoke(&registry, "llm::invoke", input)
        .await
        .expect("bare field map should be accepted");

    assert_eq!(
        result.get("risk_level").and_then(Value::as_str),
        Some("high")
    );
    assert_eq!(result.get("score").and_then(Value::as_f64), Some(0.2));
}

/// A bare map of *sample values* is ambiguous — a bare string could mean a type
/// or an enum — so it is rejected with a message showing the expected shape.
#[tokio::test]
async fn invoke_rejects_a_bare_map_of_sample_values() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "prompt": "review this diff",
        "schema": { "risk_level": "high", "score": 0.2 }
    });

    let err = invoke(&registry, "llm::invoke", input)
        .await
        .expect_err("sample values are ambiguous and must be rejected");
    let message = err.to_string();
    assert!(
        message.contains("properties"),
        "error should show the expected shape, got: {message}"
    );
}

// -- argument handling -------------------------------------------------------

#[tokio::test]
async fn invoke_without_prompt_fails() {
    let (_server, registry) = make_mock_registry().await;

    let err = invoke(&registry, "llm::invoke", json!({ "schema": schema() }))
        .await
        .expect_err("missing 'prompt' should fail");
    assert!(
        err.to_string().contains("prompt"),
        "error should name the missing field, got: {err}"
    );
}

/// An unrepresentable schema must fail with a message naming the field, not
/// silently send a mis-shaped output format to the model.
#[tokio::test]
async fn invoke_rejects_an_unsupported_schema() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "prompt": "review this diff",
        "schema": { "type": "object", "properties": { "ratio": { "type": "decimal" } } }
    });

    let err = invoke(&registry, "llm::invoke", input)
        .await
        .expect_err("an unsupported 'type' should fail");
    let message = err.to_string();
    assert!(
        message.contains("ratio") && message.contains("decimal"),
        "error should name the field and the bad type, got: {message}"
    );
}

#[tokio::test]
async fn invoke_accepts_args_nesting() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({ "args": { "prompt": "free-text-completion-prompt" } });

    invoke(&registry, "llm::invoke", input)
        .await
        .expect("fields nested under 'args' should resolve");
}

// -- fallback ----------------------------------------------------------------

/// `llm::invoke_with_fallback` must pin BAML's fallback client rather than
/// re-implementing a tier loop.
#[tokio::test]
async fn fallback_pins_the_baml_fallback_client() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({ "prompt": "free-text-completion-prompt", "tiers": ["anthropic"] });

    let result = invoke(&registry, "llm::invoke_with_fallback", input)
        .await
        .expect("llm::invoke_with_fallback should succeed");

    assert_eq!(result.get("client").and_then(Value::as_str), Some("Auto"));
    assert_eq!(
        result.get("content").and_then(Value::as_str),
        Some("Paris is the capital of France.")
    );
}

// -- stream ------------------------------------------------------------------

#[tokio::test]
async fn stream_returns_content_and_chunks() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({ "prompt": "free-text-completion-prompt" });

    let result = invoke(&registry, "llm::stream", input)
        .await
        .expect("llm::stream should succeed");

    assert_eq!(result.get("streaming"), Some(&Value::Bool(true)));
    let chunks = result
        .get("chunks")
        .and_then(Value::as_array)
        .expect("llm::stream must return 'chunks'");
    assert!(!chunks.is_empty(), "chunks must not be empty");
    let assembled: String = chunks.iter().filter_map(Value::as_str).collect::<String>();
    assert_eq!(
        assembled,
        result
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
    );
}

/// Streaming honours a caller schema too — it is the same BAML function.
#[tokio::test]
async fn stream_honours_a_caller_supplied_schema() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({ "prompt": "review this diff", "schema": schema() });

    let result = invoke(&registry, "llm::stream", input)
        .await
        .expect("llm::stream should succeed");

    assert_eq!(
        result.get("risk_level").and_then(Value::as_str),
        Some("high")
    );
    assert_eq!(result.get("streaming"), Some(&Value::Bool(true)));
}
