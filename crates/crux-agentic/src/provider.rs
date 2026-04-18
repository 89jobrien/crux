use cruxai_core::prelude::CruxErr;
use serde::{Deserialize, Serialize};

/// Domain types that cross the replay boundary — must be Serialize + DeserializeOwned.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    /// The generated text content.
    pub text: String,
    /// Identifies which provider/model produced this response, e.g. "anthropic/claude-sonnet-4-6".
    pub provider: String,
    /// Optional provider-specific metadata (usage stats, model name, etc.).
    /// Passes through verbatim in the handler's output JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// Port for LLM completion providers.
///
/// Uses RPITIT (`impl Future`) so no `async_trait` macro is needed.
pub trait LlmProvider: Send + Sync + 'static {
    fn complete(
        &self,
        req: LlmRequest,
    ) -> impl std::future::Future<Output = Result<LlmResponse, CruxErr>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_max_tokens_is_1024() {
        let req = LlmRequest::default();
        assert_eq!(req.max_tokens, 1024);
    }

    #[test]
    fn llm_request_roundtrips_serde() {
        let req = LlmRequest {
            prompt: "hello".into(),
            system: Some("be helpful".into()),
            max_tokens: 512,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: LlmRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.prompt, "hello");
        assert_eq!(back.max_tokens, 512);
    }

    #[test]
    fn llm_response_roundtrips_serde() {
        let resp = LlmResponse {
            text: "answer".into(),
            provider: "openai/gpt-4o".into(),
            metadata: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: LlmResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text, "answer");
        assert_eq!(back.provider, "openai/gpt-4o");
    }
}
