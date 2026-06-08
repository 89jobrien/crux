use crate::adapters::{AnthropicAdapter, OllamaAdapter, OpenAiAdapter};
use crate::error::opt_str;
use crate::provider::LlmProvider;
use crate::provider::LlmRequest;
use crux_model::{ProviderModelId, ProviderModelRef, Vendor};
use crux_runtime::prelude::CruxErr;
use crux_script::HandlerRegistry;
use serde_json::{Value, json};

const DEFAULT_MAX_TOKENS: u32 = 1024;
const DEFAULT_MODEL: &str = "gpt-4o-mini";
const DEFAULT_SYSTEM: &str = "You are a helpful assistant.";
const DEFAULT_BASE_URL_ANTHROPIC: &str = "https://api.anthropic.com";
const DEFAULT_BASE_URL_OLLAMA: &str = "http://localhost:11434";
const DEFAULT_BASE_URL_OPENAI: &str = "https://api.openai.com";

struct ParsedInput {
    prompt: String,
    vendor: Vendor,
    model_ref: ProviderModelRef,
    system: String,
    max_tokens: u32,
    api_key: String,
}

fn parse_llm_input(input: &Value, handler: &str) -> Result<ParsedInput, CruxErr> {
    let prompt = input
        .get("prompt")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CruxErr::step_failed(handler, "missing 'prompt' field"))?
        .to_string();

    let vendor = opt_str(input, "provider")
        .unwrap_or("openai")
        .parse::<Vendor>()
        .unwrap_or(Vendor::OpenAi);
    let model_str = opt_str(input, "model").unwrap_or(DEFAULT_MODEL);
    let model_ref = ProviderModelId::parse_lenient(vendor, model_str);
    let system = opt_str(input, "system")
        .unwrap_or(DEFAULT_SYSTEM)
        .to_string();
    let max_tokens = input
        .get("args")
        .and_then(|a| a.get("max_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_MAX_TOKENS as u64) as u32;
    let api_key = opt_str(input, "api_key")
        .map(str::to_string)
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
        .unwrap_or_default();

    Ok(ParsedInput {
        prompt,
        vendor,
        model_ref,
        system,
        max_tokens,
        api_key,
    })
}

// TODO(review): consider taking &str for api_key — fallback handler clones
//   per vendor attempt; clone inside only when adapter constructor needs owned
async fn dispatch_llm(
    vendor: Vendor,
    api_key: String,
    model_ref: ProviderModelRef,
    base_url_override: Option<&str>,
    req: LlmRequest,
) -> Result<crate::provider::LlmResponse, CruxErr> {
    match vendor {
        Vendor::Anthropic => {
            let base_url = base_url_override
                .unwrap_or(DEFAULT_BASE_URL_ANTHROPIC)
                .to_string();
            AnthropicAdapter::new(api_key, model_ref, base_url)
                .complete(req)
                .await
        }
        Vendor::Ollama => {
            let base_url = base_url_override
                .unwrap_or(DEFAULT_BASE_URL_OLLAMA)
                .to_string();
            OllamaAdapter::new(model_ref, base_url).complete(req).await
        }
        _ => {
            let base_url = base_url_override
                .unwrap_or(DEFAULT_BASE_URL_OPENAI)
                .to_string();
            OpenAiAdapter::new(api_key, model_ref, base_url)
                .complete(req)
                .await
        }
    }
}

fn merge_metadata(out: &mut Value, resp: &crate::provider::LlmResponse) {
    if let Some(meta) = &resp.metadata
        && let (Some(map), Some(meta_obj)) = (out.as_object_mut(), meta.as_object())
    {
        for (k, v) in meta_obj {
            map.insert(k.clone(), v.clone());
        }
    }
}

/// Register the `llm::stream` handler.
///
/// **Stub implementation**: emits the full response as a single output rather than
/// streaming deltas.  Real streaming requires an async-stream variant on `LlmProvider`
/// (tracked in issue #21).  The handler is wire-compatible: callers get `content`,
/// `provider`, and `streaming: false` in the output.
pub fn register_stream(registry: &mut HandlerRegistry) {
    registry.handler_value("llm::stream", |input: Value| async move {
        let p = parse_llm_input(&input, "llm::stream")?;
        let base_url = opt_str(&input, "base_url");
        let req = LlmRequest {
            prompt: p.prompt,
            system: Some(p.system),
            max_tokens: p.max_tokens,
        };

        let resp = dispatch_llm(p.vendor, p.api_key, p.model_ref, base_url, req).await?;

        let mut out = json!({
            "content": resp.text,
            "provider": resp.provider,
            "streaming": false,
        });
        merge_metadata(&mut out, &resp);
        Ok(out)
    });
}

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value("llm::invoke", |input: Value| async move {
        let p = parse_llm_input(&input, "llm::invoke")?;
        let base_url = opt_str(&input, "base_url");
        let req = LlmRequest {
            prompt: p.prompt,
            system: Some(p.system),
            max_tokens: p.max_tokens,
        };

        let resp = dispatch_llm(p.vendor, p.api_key, p.model_ref, base_url, req).await?;

        let mut out = json!({ "content": resp.text, "provider": resp.provider });
        merge_metadata(&mut out, &resp);
        Ok(out)
    });
}

/// Register the `llm::invoke_with_fallback` handler.
///
/// Tries providers in tier order. On failure, falls through to the next
/// provider. The `tiers` field in input specifies the order as an array of
/// vendor strings (e.g. `["anthropic", "openai", "ollama"]`). If omitted,
/// defaults to `["anthropic", "openai"]`.
pub fn register_fallback(registry: &mut HandlerRegistry) {
    registry.handler_value("llm::invoke_with_fallback", |input: Value| async move {
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CruxErr::step_failed("llm::invoke_with_fallback", "missing 'prompt' field")
            })?
            .to_string();

        let tiers: Vec<Vendor> = input
            .get("tiers")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str()?.parse::<Vendor>().ok())
                    .collect()
            })
            .unwrap_or_else(|| vec![Vendor::Anthropic, Vendor::OpenAi]);

        let model_str = opt_str(&input, "model").unwrap_or(DEFAULT_MODEL);
        let system = opt_str(&input, "system")
            .unwrap_or(DEFAULT_SYSTEM)
            .to_string();
        let max_tokens = input
            .get("args")
            .and_then(|a| a.get("max_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_MAX_TOKENS as u64) as u32;
        let api_key = opt_str(&input, "api_key")
            .map(str::to_string)
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .unwrap_or_default();
        let base_url_override = opt_str(&input, "base_url");

        let mut last_err =
            CruxErr::step_failed("llm::invoke_with_fallback", "no providers configured");

        for vendor in &tiers {
            let model_ref = ProviderModelId::parse_lenient(*vendor, model_str);
            let req = LlmRequest {
                prompt: prompt.clone(),
                system: Some(system.clone()),
                max_tokens,
            };
            let result =
                dispatch_llm(*vendor, api_key.clone(), model_ref, base_url_override, req).await;
            match result {
                Ok(resp) => {
                    let mut out = json!({ "content": resp.text, "provider": resp.provider });
                    merge_metadata(&mut out, &resp);
                    return Ok(out);
                }
                Err(e) => {
                    eprintln!("[llm::invoke_with_fallback] {vendor:?} failed: {e}, trying next");
                    last_err = e;
                }
            }
        }
        Err(last_err)
    });
}
