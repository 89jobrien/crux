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

async fn dispatch_llm(
    vendor: Vendor,
    api_key: &str,
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

async fn dispatch_llm_stream(
    vendor: Vendor,
    api_key: &str,
    model_ref: ProviderModelRef,
    base_url_override: Option<&str>,
    req: LlmRequest,
) -> Result<(Vec<String>, String), CruxErr> {
    let base_url = base_url_override.unwrap_or(match vendor {
        Vendor::Anthropic => DEFAULT_BASE_URL_ANTHROPIC,
        Vendor::Ollama => DEFAULT_BASE_URL_OLLAMA,
        _ => DEFAULT_BASE_URL_OPENAI,
    });
    let base_url = base_url.trim_end_matches('/');
    let provider = format!(
        "{}/{}",
        vendor.to_string().to_lowercase(),
        model_ref.provider_id
    );
    let system = req.system.unwrap_or_else(|| DEFAULT_SYSTEM.into());
    let (url, body) = if vendor == Vendor::Anthropic {
        (
            format!("{base_url}/v1/messages"),
            json!({
                "model": model_ref.provider_id,
                "max_tokens": req.max_tokens,
                "stream": true,
                "system": system,
                "messages": [{"role": "user", "content": req.prompt}]
            }),
        )
    } else {
        (
            format!("{base_url}/v1/chat/completions"),
            json!({
                "model": model_ref.provider_id,
                "max_tokens": req.max_tokens,
                "stream": true,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": req.prompt}
                ]
            }),
        )
    };
    let client = reqwest::Client::new();
    let mut request = client.post(url).json(&body);
    if !api_key.is_empty() {
        request = match vendor {
            Vendor::Anthropic => request
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01"),
            Vendor::Ollama => request,
            _ => request.bearer_auth(api_key),
        };
    }
    let mut response = request
        .send()
        .await
        .map_err(|error| CruxErr::step_failed("llm::stream", format!("HTTP error: {error}")))?;
    if !response.status().is_success() {
        return Err(CruxErr::step_failed(
            "llm::stream",
            format!("HTTP status {}", response.status()),
        ));
    }

    let mut pending = Vec::new();
    let mut deltas = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| CruxErr::step_failed("llm::stream", error.to_string()))?
    {
        pending.extend_from_slice(&chunk);
        while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
            let line = pending.drain(..=newline).collect::<Vec<_>>();
            parse_stream_line(vendor, &line, &mut deltas)?;
        }
    }
    if !pending.is_empty() {
        parse_stream_line(vendor, &pending, &mut deltas)?;
    }
    Ok((deltas, provider))
}

fn parse_stream_line(
    vendor: Vendor,
    bytes: &[u8],
    deltas: &mut Vec<String>,
) -> Result<(), CruxErr> {
    let line = std::str::from_utf8(bytes)
        .map_err(|error| CruxErr::step_failed("llm::stream", error.to_string()))?
        .trim();
    if !line.starts_with("data:") && !line.starts_with('{') {
        return Ok(());
    }
    let data = line.strip_prefix("data:").map(str::trim).unwrap_or(line);
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let event: Value = serde_json::from_str(data).map_err(|error| {
        CruxErr::step_failed("llm::stream", format!("invalid stream event: {error}"))
    })?;
    let delta = match vendor {
        Vendor::Anthropic => event.pointer("/delta/text"),
        _ => event.pointer("/choices/0/delta/content"),
    };
    if let Some(delta) = delta.and_then(Value::as_str)
        && !delta.is_empty()
    {
        deltas.push(delta.to_owned());
    }
    Ok(())
}

/// Register the `llm::stream` handler.
///
/// Provider deltas are consumed incrementally and returned as ordered chunks alongside
/// the assembled content.
pub fn register_stream(registry: &mut HandlerRegistry) {
    registry.handler_value("llm::stream", |input: Value| async move {
        let p = parse_llm_input(&input, "llm::stream")?;
        let base_url = opt_str(&input, "base_url");
        let req = LlmRequest {
            prompt: p.prompt,
            system: Some(p.system),
            max_tokens: p.max_tokens,
        };

        let (chunks, provider) =
            dispatch_llm_stream(p.vendor, &p.api_key, p.model_ref, base_url, req).await?;
        let content = chunks.concat();
        Ok(json!({
            "content": content,
            "chunks": chunks,
            "provider": provider,
            "streaming": true,
        }))
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

        let resp = dispatch_llm(p.vendor, &p.api_key, p.model_ref, base_url, req).await?;

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
            let result = dispatch_llm(*vendor, &api_key, model_ref, base_url_override, req).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_accepts_a_borrowed_api_key() {
        let api_key = String::from("borrowed-key");
        let model = ProviderModelId::parse_lenient(Vendor::Ollama, "test-model");
        let request = LlmRequest {
            prompt: "test".into(),
            system: None,
            max_tokens: 1,
        };

        let future = dispatch_llm(Vendor::Ollama, api_key.as_str(), model, None, request);
        drop(future);
        assert_eq!(api_key, "borrowed-key");
    }
}
