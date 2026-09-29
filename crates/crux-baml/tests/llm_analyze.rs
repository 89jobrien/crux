//! Mock-server integration tests for the `llm::analyze` and `llm::confidence`
//! handlers. No API keys or local Ollama instance required.

mod mock_baml;

use crate::mock_baml::{MockBamlServer, default_responses};
use crux_baml::analyze::register_with;
use crux_script::{HandlerRegistry, handler_output::HandlerOutput};
use serde_json::{Value, json};

async fn make_mock_registry() -> (MockBamlServer, HandlerRegistry) {
    let server = MockBamlServer::start(default_responses()).await;
    let client_registry = server.registry();
    let mut registry = HandlerRegistry::new();
    register_with(&mut registry, Some(client_registry));
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

// -- llm::analyze ------------------------------------------------------------

#[tokio::test]
async fn analyze_returns_summary_findings_and_recommendation() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "subject": "cargo deny advisories report",
        "evidence": "warning[RUSTSEC-2024-0001]: openssl 0.9.1 severity: high",
        "focus": "security advisories"
    });

    let result = invoke(&registry, "llm::analyze", input)
        .await
        .expect("llm::analyze should succeed");

    assert!(
        result.get("summary").and_then(Value::as_str).is_some(),
        "result must contain 'summary'"
    );
    assert!(
        result
            .get("recommendation")
            .and_then(Value::as_str)
            .is_some(),
        "result must contain 'recommendation'"
    );

    let findings = result
        .get("findings")
        .and_then(Value::as_array)
        .expect("result must contain 'findings' array");
    assert!(!findings.is_empty(), "expected at least one finding");

    for finding in findings {
        assert!(finding.get("title").is_some(), "finding needs 'title'");
        assert!(finding.get("detail").is_some(), "finding needs 'detail'");
        let severity = finding
            .get("severity")
            .and_then(Value::as_str)
            .expect("finding needs 'severity'");
        assert!(
            matches!(severity, "blocking" | "warning" | "info"),
            "severity must be lowercased, got: {severity}"
        );
    }
}

/// The analyze step must report confidence so `route_on_confidence` can consume it.
#[tokio::test]
async fn analyze_reports_step_confidence() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "subject": "cargo deny advisories report",
        "evidence": "warning[RUSTSEC-2024-0001]: openssl 0.9.1 severity: high"
    });

    let result = invoke(&registry, "llm::analyze", input)
        .await
        .expect("llm::analyze should succeed");

    let confidence = result
        .confidence
        .expect("analyze step must report a confidence score");
    assert!(
        (0.0..=1.0).contains(&confidence),
        "confidence must be within [0.0, 1.0], got: {confidence}"
    );
    assert_eq!(confidence, result.confidence_or_default());

    // The payload keeps the model's full f64 precision; the step confidence is the
    // same value narrowed to f32, since step confidence is defined as f32.
    let payload_confidence = result
        .get("confidence")
        .and_then(Value::as_f64)
        .expect("result must contain 'confidence'");
    assert!((payload_confidence - f64::from(confidence)).abs() < 1e-6);
}

#[tokio::test]
async fn analyze_accepts_args_nesting() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "args": {
            "subject": "cargo deny advisories report",
            "evidence": "warning[RUSTSEC-2024-0001]: openssl 0.9.1 severity: high"
        }
    });

    invoke(&registry, "llm::analyze", input)
        .await
        .expect("fields nested under 'args' should resolve");
}

#[tokio::test]
async fn analyze_without_evidence_fails() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({ "subject": "cargo deny advisories report" });

    let err = invoke(&registry, "llm::analyze", input)
        .await
        .expect_err("missing 'evidence' should fail");
    assert!(
        err.to_string().contains("evidence"),
        "error should name the missing field, got: {err}"
    );
}

// -- llm::confidence ---------------------------------------------------------

#[tokio::test]
async fn confidence_scores_claim_against_evidence() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "claim": "The working tree has no unstaged changes.",
        "evidence": "git status --porcelain produced no output.",
        "criteria": ["Is the evidence direct and unambiguous?"]
    });

    let result = invoke(&registry, "llm::confidence", input)
        .await
        .expect("llm::confidence should succeed");

    let score = result
        .get("score")
        .and_then(Value::as_f64)
        .expect("result must contain 'score'");
    assert!(
        (0.0..=1.0).contains(&score),
        "score must be in range: {score}"
    );

    assert_eq!(
        result.get("level").and_then(Value::as_str),
        Some("high"),
        "level must be lowercased"
    );
    assert!(
        result.get("reasoning").and_then(Value::as_str).is_some(),
        "result must contain 'reasoning'"
    );
    assert!(
        result
            .get("factors")
            .and_then(Value::as_array)
            .is_some_and(|f| !f.is_empty()),
        "result must contain a non-empty 'factors' array"
    );
}

/// The confidence step's score must become the step confidence, which is the
/// whole point of routing on it.
#[tokio::test]
async fn confidence_score_becomes_step_confidence() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "claim": "The working tree has no unstaged changes.",
        "evidence": "git status --porcelain produced no output."
    });

    let result = invoke(&registry, "llm::confidence", input)
        .await
        .expect("llm::confidence should succeed");

    let score = result
        .get("score")
        .and_then(Value::as_f64)
        .expect("result must contain 'score'");
    assert_eq!(
        result.confidence,
        Some(score as f32),
        "step confidence must mirror the reported score"
    );
}

/// `criteria` is optional — an empty list must still be accepted.
#[tokio::test]
async fn confidence_criteria_are_optional() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "claim": "All tests pass.",
        "evidence": "Only cargo build was run."
    });

    invoke(&registry, "llm::confidence", input)
        .await
        .expect("omitted 'criteria' should be accepted");
}

#[tokio::test]
async fn confidence_without_claim_fails() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({ "evidence": "git status --porcelain produced no output." });

    let err = invoke(&registry, "llm::confidence", input)
        .await
        .expect_err("missing 'claim' should fail");
    assert!(
        err.to_string().contains("claim"),
        "error should name the missing field, got: {err}"
    );
}
