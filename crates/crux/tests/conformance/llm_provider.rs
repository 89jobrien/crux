/// Conformance tests: LlmProvider port — stub adapter satisfies the trait contract.
///
/// Verifies: a user-defined type can implement LlmProvider, the port types are
/// Serialize+DeserializeOwned, and serde semantics match the spec.
use crux::prelude::CruxErr;
use crux_agentic::provider::{LlmProvider, LlmRequest, LlmResponse};

// ── stub adapter ──────────────────────────────────────────────────────────────

struct EchoProvider {
    label: &'static str,
}

impl LlmProvider for EchoProvider {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, CruxErr> {
        Ok(LlmResponse {
            text: format!("echo: {}", req.prompt),
            provider: self.label.to_string(),
            metadata: None,
        })
    }
}

// ── contract: complete() returns Ok with correct fields ──────────────────────

#[tokio::test]
async fn conformance_llm_provider_complete_returns_ok() {
    let p = EchoProvider { label: "test/echo" };
    let req = LlmRequest {
        prompt: "hello".into(),
        system: None,
        max_tokens: 64,
    };
    let resp = p.complete(req).await.unwrap();
    assert_eq!(resp.text, "echo: hello");
    assert_eq!(resp.provider, "test/echo");
}

#[tokio::test]
async fn conformance_llm_provider_metadata_none_when_not_set() {
    let p = EchoProvider { label: "test/echo" };
    let resp = p.complete(LlmRequest::default()).await.unwrap();
    assert!(resp.metadata.is_none());
}

// ── contract: LlmRequest serde roundtrip ────────────────────────────────────

#[test]
fn conformance_llm_request_serde_roundtrip() {
    let req = LlmRequest {
        prompt: "what is Rust?".into(),
        system: Some("be concise".into()),
        max_tokens: 256,
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: LlmRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(back.prompt, req.prompt);
    assert_eq!(back.system, req.system);
    assert_eq!(back.max_tokens, req.max_tokens);
}

// ── contract: LlmResponse serde roundtrip ───────────────────────────────────

#[test]
fn conformance_llm_response_serde_roundtrip() {
    let resp = LlmResponse {
        text: "Rust is a systems language.".into(),
        provider: "anthropic/claude-sonnet-4-6".into(),
        metadata: Some(serde_json::json!({"tokens": 42})),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: LlmResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.text, resp.text);
    assert_eq!(back.provider, resp.provider);
    assert_eq!(back.metadata, resp.metadata);
}

// ── contract: metadata absent from JSON when None (skip_serializing_if) ──────

#[test]
fn conformance_llm_response_metadata_absent_when_none() {
    let resp = LlmResponse {
        text: "ok".into(),
        provider: "test".into(),
        metadata: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(
        !json.contains("metadata"),
        "metadata field must be absent in JSON when None"
    );
}

// ── contract: LlmRequest::default() has max_tokens=1024 ──────────────────────

#[test]
fn conformance_llm_request_default_max_tokens_is_1024() {
    let req = LlmRequest::default();
    assert_eq!(req.max_tokens, 1024);
    assert!(req.prompt.is_empty());
    assert!(req.system.is_none());
}
