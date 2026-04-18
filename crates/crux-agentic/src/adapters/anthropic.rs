use cruxai_core::prelude::CruxErr;
use serde_json::json;

use crate::provider::{LlmProvider, LlmRequest, LlmResponse};

pub struct AnthropicAdapter {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
}

impl AnthropicAdapter {
    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            model: "claude-sonnet-4-6".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
        }
    }

    pub fn new(api_key: impl Into<String>, model: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.into(),
        }
    }
}

impl LlmProvider for AnthropicAdapter {
    fn complete(
        &self,
        req: LlmRequest,
    ) -> impl std::future::Future<Output = Result<LlmResponse, CruxErr>> + Send {
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let base_url = self.base_url.clone();

        async move {
            let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
            let system = req.system.unwrap_or_else(|| "You are a helpful assistant.".into());
            let body = json!({
                "model": model,
                "max_tokens": req.max_tokens,
                "system": system,
                "messages": [
                    {"role": "user", "content": req.prompt}
                ]
            });

            let client = reqwest::Client::new();
            let resp = client
                .post(&url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await
                .map_err(|e| CruxErr::step_failed("llm::complete", format!("HTTP error: {e}")))?;

            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| CruxErr::step_failed("llm::complete", format!("JSON decode error: {e}")))?;

            let text = json["content"][0]["text"]
                .as_str()
                .ok_or_else(|| CruxErr::step_failed("llm::complete", "unexpected response shape"))?
                .to_string();

            Ok(LlmResponse {
                text,
                provider: format!("anthropic/{model}"),
                metadata: Some(serde_json::json!({
                    "model": json["model"],
                    "usage": json["usage"],
                })),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_uses_defaults() {
        let adapter = AnthropicAdapter::from_env();
        assert_eq!(adapter.base_url, "https://api.anthropic.com");
        assert_eq!(adapter.model, "claude-sonnet-4-6");
    }

    #[test]
    fn new_sets_fields() {
        let adapter = AnthropicAdapter::new("key", "claude-3-5-sonnet", "https://custom.example.com");
        assert_eq!(adapter.api_key, "key");
        assert_eq!(adapter.model, "claude-3-5-sonnet");
        assert_eq!(adapter.base_url, "https://custom.example.com");
    }
}
