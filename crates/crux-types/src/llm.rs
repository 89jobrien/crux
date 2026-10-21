//! Provider-neutral LLM request and response wire types.
//!
//! These types are the serializable boundary between the crux agentic layer and
//! any LLM provider adapter. They are defined here so that crates without an
//! async runtime dependency (e.g. `crux-types` consumers such as `minibox`) can
//! work with LLM trace data without pulling in the full `crux-agentic` stack.

use serde::{Deserialize, Serialize};

/// A provider-neutral LLM completion request.
///
/// Implements [`Default`] with `max_tokens = 1024` and empty `prompt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmRequest {
    pub prompt: String,
    pub system: Option<String>,
    pub max_tokens: u32,
}

impl Default for LlmRequest {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            system: None,
            max_tokens: 1024,
        }
    }
}

/// A provider-neutral LLM completion response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmResponse {
    /// The generated text content.
    pub text: String,
    /// Identifies which provider/model produced this response,
    /// e.g. `"anthropic/claude-sonnet-4-6"`.
    pub provider: String,
    /// Optional provider-specific metadata (usage stats, model name, etc.).
    /// Passes through verbatim in the handler's output JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_max_tokens_is_1024() {
        let req = LlmRequest::default();
        assert_eq!(req.max_tokens, 1024);
        assert!(req.prompt.is_empty());
        assert!(req.system.is_none());
    }

    #[test]
    fn llm_request_roundtrips_serde() {
        let req = LlmRequest {
            prompt: "hello".into(),
            system: Some("be helpful".into()),
            max_tokens: 512,
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let back: LlmRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, req);
    }

    #[test]
    fn llm_request_roundtrips_serde_no_system() {
        let req = LlmRequest {
            prompt: "summarize this".into(),
            system: None,
            max_tokens: 256,
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let back: LlmRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, req);
    }

    #[test]
    fn llm_response_roundtrips_serde() {
        let resp = LlmResponse {
            text: "answer".into(),
            provider: "openai/gpt-4o".into(),
            metadata: None,
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        let back: LlmResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, resp);
    }

    #[test]
    fn llm_response_roundtrips_serde_with_metadata() {
        let resp = LlmResponse {
            text: "42".into(),
            provider: "anthropic/claude-sonnet-4-6".into(),
            metadata: Some(serde_json::json!({"input_tokens": 10, "output_tokens": 1})),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        let back: LlmResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, resp);
    }

    #[test]
    fn llm_response_metadata_omitted_when_none() {
        let resp = LlmResponse {
            text: "hi".into(),
            provider: "test/model".into(),
            metadata: None,
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(
            !json.contains("metadata"),
            "metadata field must be absent when None"
        );
    }
}
