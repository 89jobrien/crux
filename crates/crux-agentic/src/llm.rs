// TODO(#74): multi-provider LLM fallback — add a Router that tries providers in
//   tier order (Anthropic -> OpenAI -> Ollama) with automatic failover (cf. devkit)

use crate::adapters::{AnthropicAdapter, OllamaAdapter, OpenAiAdapter};
use crate::error::opt_str;
use crate::provider::LlmProvider;
use crate::provider::LlmRequest;
use crux_model::{ProviderModelId, Vendor};
use crux_runtime::prelude::CruxErr;
use crux_script::HandlerRegistry;
use serde_json::{Value, json};

/// Register the `llm::stream` handler.
///
/// **Stub implementation**: emits the full response as a single output rather than
/// streaming deltas.  Real streaming requires an async-stream variant on `LlmProvider`
/// (tracked in issue #21).  The handler is wire-compatible: callers get `content`,
/// `provider`, and `streaming: false` in the output.
pub fn register_stream(registry: &mut HandlerRegistry) {
    registry.handler_value("llm::stream", |input: Value| async move {
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CruxErr::step_failed("llm::stream", "missing 'prompt' field"))?
            .to_string();

        let vendor = opt_str(&input, "provider")
            .unwrap_or("openai")
            .parse::<Vendor>()
            .unwrap_or(Vendor::OpenAi);
        let model_str = opt_str(&input, "model").unwrap_or("gpt-4o-mini");
        let model_ref = ProviderModelId::parse_lenient(vendor, model_str);
        let system = opt_str(&input, "system")
            .unwrap_or("You are a helpful assistant.")
            .to_string();
        let max_tokens = input
            .get("args")
            .and_then(|a| a.get("max_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1024) as u32;
        let api_key = opt_str(&input, "api_key")
            .map(str::to_string)
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .unwrap_or_default();

        let req = LlmRequest {
            prompt,
            system: Some(system),
            max_tokens,
        };

        let resp = match vendor {
            Vendor::Anthropic => {
                let base_url = opt_str(&input, "base_url")
                    .unwrap_or("https://api.anthropic.com")
                    .to_string();
                AnthropicAdapter::new(api_key, model_ref, base_url)
                    .complete(req)
                    .await?
            }
            Vendor::Ollama => {
                let base_url = opt_str(&input, "base_url")
                    .unwrap_or("http://localhost:11434")
                    .to_string();
                OllamaAdapter::new(model_ref, base_url)
                    .complete(req)
                    .await?
            }
            _ => {
                let base_url = opt_str(&input, "base_url")
                    .unwrap_or("https://api.openai.com")
                    .to_string();
                OpenAiAdapter::new(api_key, model_ref, base_url)
                    .complete(req)
                    .await?
            }
        };

        let mut out = json!({
            "content": resp.text,
            "provider": resp.provider,
            "streaming": false,
        });
        if let Some(meta) = resp.metadata
            && let (Some(map), Some(meta_obj)) = (out.as_object_mut(), meta.as_object())
        {
            for (k, v) in meta_obj {
                map.insert(k.clone(), v.clone());
            }
        }
        Ok(out)
    });
}

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value("llm::invoke", |input: Value| async move {
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CruxErr::step_failed("llm::invoke", "missing 'prompt' field"))?
            .to_string();

        let vendor = opt_str(&input, "provider")
            .unwrap_or("openai")
            .parse::<Vendor>()
            .unwrap_or(Vendor::OpenAi);
        let model_str = opt_str(&input, "model").unwrap_or("gpt-4o-mini");
        let model_ref = ProviderModelId::parse_lenient(vendor, model_str);
        let system = opt_str(&input, "system")
            .unwrap_or("You are a helpful assistant.")
            .to_string();
        let max_tokens = input
            .get("args")
            .and_then(|a| a.get("max_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1024) as u32;
        let api_key = opt_str(&input, "api_key")
            .map(str::to_string)
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .unwrap_or_default();

        let req = LlmRequest {
            prompt,
            system: Some(system),
            max_tokens,
        };

        let resp = match vendor {
            Vendor::Anthropic => {
                let base_url = opt_str(&input, "base_url")
                    .unwrap_or("https://api.anthropic.com")
                    .to_string();
                AnthropicAdapter::new(api_key, model_ref, base_url)
                    .complete(req)
                    .await?
            }
            Vendor::Ollama => {
                let base_url = opt_str(&input, "base_url")
                    .unwrap_or("http://localhost:11434")
                    .to_string();
                OllamaAdapter::new(model_ref, base_url)
                    .complete(req)
                    .await?
            }
            _ => {
                let base_url = opt_str(&input, "base_url")
                    .unwrap_or("https://api.openai.com")
                    .to_string();
                OpenAiAdapter::new(api_key, model_ref, base_url)
                    .complete(req)
                    .await?
            }
        };

        let mut out = json!({ "content": resp.text, "provider": resp.provider });
        if let Some(meta) = resp.metadata
            && let (Some(map), Some(meta_obj)) = (out.as_object_mut(), meta.as_object())
        {
            for (k, v) in meta_obj {
                map.insert(k.clone(), v.clone());
            }
        }
        Ok(out)
    });
}
