use cruxai_core::prelude::CruxErr;
use cruxai_model::{ProviderModelId, ProviderModelRef, Vendor};
use serde_json::json;

use crate::provider::{LlmProvider, LlmRequest, LlmResponse};

pub struct OpenAiAdapter {
    pub api_key: String,
    pub model: ProviderModelRef,
    pub base_url: String,
}

impl OpenAiAdapter {
    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            model: ProviderModelId::parse_lenient(Vendor::OpenAi, "gpt-4o-mini"),
            base_url: "https://api.openai.com".to_string(),
        }
    }

    pub fn new(
        api_key: impl Into<String>,
        model: ProviderModelRef,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model,
            base_url: base_url.into(),
        }
    }
}

impl LlmProvider for OpenAiAdapter {
    fn complete(
        &self,
        req: LlmRequest,
    ) -> impl std::future::Future<Output = Result<LlmResponse, CruxErr>> + Send {
        let api_key = self.api_key.clone();
        let model = self.model.provider_id.clone();
        let base_url = self.base_url.clone();

        async move {
            let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
            let system = req
                .system
                .unwrap_or_else(|| "You are a helpful assistant.".into());
            let body = json!({
                "model": model,
                "max_tokens": req.max_tokens,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": req.prompt}
                ]
            });

            let client = reqwest::Client::new();
            let mut http_req = client.post(&url).json(&body);
            if !api_key.is_empty() {
                http_req = http_req.bearer_auth(&api_key);
            }

            let resp = http_req
                .send()
                .await
                .map_err(|e| CruxErr::step_failed("llm::invoke", format!("HTTP error: {e}")))?;

            let json: serde_json::Value = resp.json().await.map_err(|e| {
                CruxErr::step_failed("llm::invoke", format!("JSON decode error: {e}"))
            })?;

            let text = json["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(|| CruxErr::step_failed("llm::invoke", "unexpected response shape"))?
                .to_string();

            Ok(LlmResponse {
                text,
                provider: format!("openai/{model}"),
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
        let adapter = OpenAiAdapter::from_env();
        assert_eq!(adapter.base_url, "https://api.openai.com");
        assert_eq!(adapter.model.provider_id, "gpt-4o-mini");
    }

    #[test]
    fn new_sets_fields() {
        let model = ProviderModelId::parse_lenient(Vendor::OpenAi, "gpt-4o");
        let adapter = OpenAiAdapter::new("key", model, "https://custom.example.com");
        assert_eq!(adapter.api_key, "key");
        assert_eq!(adapter.model.provider_id, "gpt-4o");
        assert_eq!(adapter.base_url, "https://custom.example.com");
    }
}
