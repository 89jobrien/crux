use crux_model::{ProviderModelId, ProviderModelRef, Vendor};
use crux_runtime::prelude::CruxErr;
use serde_json::json;

use crate::provider::{LlmProvider, LlmRequest, LlmResponse};

pub struct OllamaAdapter {
    pub model: ProviderModelRef,
    pub base_url: String,
}

impl OllamaAdapter {
    pub fn from_env() -> Self {
        let base_url = std::env::var("OLLAMA_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = ProviderModelId::parse_lenient(Vendor::Ollama, "llama3.2:latest");
        Self { model, base_url }
    }

    pub fn new(model: ProviderModelRef, base_url: impl Into<String>) -> Self {
        Self {
            model,
            base_url: base_url.into(),
        }
    }
}

impl LlmProvider for OllamaAdapter {
    fn complete(
        &self,
        req: LlmRequest,
    ) -> impl std::future::Future<Output = Result<LlmResponse, CruxErr>> + Send {
        let model_id = self.model.provider_id.clone();
        let base_url = self.base_url.clone();

        async move {
            let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
            let system = req
                .system
                .unwrap_or_else(|| "You are a helpful assistant.".into());
            let body = json!({
                "model": model_id,
                "max_tokens": req.max_tokens,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": req.prompt}
                ]
            });

            let client = reqwest::Client::new();
            let resp = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .map_err(|e| CruxErr::step_failed("llm::invoke", format!("HTTP error: {e}")))?;

            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| CruxErr::step_failed("llm::invoke", format!("JSON error: {e}")))?;

            let text = json["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(|| CruxErr::step_failed("llm::invoke", "unexpected response shape"))?
                .to_string();

            Ok(LlmResponse {
                text,
                provider: format!("ollama/{model_id}"),
                metadata: Some(json!({
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
        let adapter = OllamaAdapter::from_env();
        assert_eq!(adapter.base_url, "http://localhost:11434");
        assert_eq!(adapter.model.provider_id, "llama3.2:latest");
    }

    #[test]
    fn new_sets_fields() {
        let model = ProviderModelId::parse_lenient(Vendor::Ollama, "qwen2.5-coder:7b");
        let adapter = OllamaAdapter::new(model, "http://custom:11434");
        assert_eq!(adapter.model.provider_id, "qwen2.5-coder:7b");
        assert_eq!(adapter.base_url, "http://custom:11434");
    }
}
