use crate::adapters::{AnthropicAdapter, OllamaAdapter, OpenAiAdapter};
use crate::error::opt_str;
use crate::provider::LlmProvider;
use crate::provider::LlmRequest;
use cruxai_core::prelude::CruxErr;
use cruxai_model::{ProviderModelId, Vendor};
use cruxai_script::HandlerRegistry;
use serde_json::{Value, json};

/// Register the `llm::extract` handler.  Only compiled when the `baml` feature is enabled.
#[cfg(feature = "baml")]
pub fn register_extract(registry: &mut HandlerRegistry) {
    use crate::baml_client::async_client::B;

    registry.handler("llm::extract", |input: Value| async move {
        let function = input
            .get("function")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CruxErr::step_failed("llm::extract", "missing 'function' field"))?
            .to_string();

        let input_map = input
            .get("input")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                CruxErr::step_failed("llm::extract", "missing or non-object 'input' field")
            })?
            .clone();

        // Optional BAML client override (e.g. "Anthropic").
        let client_override = input
            .get("client")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let b = if let Some(client_name) = client_override {
            B.with_client(client_name)
        } else {
            B.clone()
        };

        match function.as_str() {
            "ExtractEntities" => {
                let text = input_map
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CruxErr::step_failed(
                            "llm::extract",
                            "ExtractEntities requires 'text' field",
                        )
                    })?
                    .to_string();

                let result = b.ExtractEntities.call(text).await.map_err(|e| {
                    CruxErr::step_failed("llm::extract", format!("BAML error: {e}"))
                })?;

                let entities: Vec<Value> = result
                    .entities
                    .iter()
                    .map(|e| {
                        json!({
                            "name": e.name,
                            "entity_type": e.entity_type,
                            "description": e.description,
                        })
                    })
                    .collect();
                Ok(json!({ "entities": entities }))
            }
            "Summarize" => {
                let text = input_map
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CruxErr::step_failed("llm::extract", "Summarize requires 'text' field")
                    })?
                    .to_string();

                let max_sentences = input_map
                    .get("max_sentences")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(3);

                let result = b.Summarize.call(text, max_sentences).await.map_err(|e| {
                    CruxErr::step_failed("llm::extract", format!("BAML error: {e}"))
                })?;

                Ok(json!({
                    "summary": result.summary,
                    "key_points": result.key_points,
                    "word_count": result.word_count,
                }))
            }
            "Classify" => {
                let text = input_map
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        CruxErr::step_failed("llm::extract", "Classify requires 'text' field")
                    })?
                    .to_string();

                let labels: Vec<String> = input_map
                    .get("labels")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        CruxErr::step_failed("llm::extract", "Classify requires 'labels' array")
                    })?
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect();

                let result = b.Classify.call(text, &labels).await.map_err(|e| {
                    CruxErr::step_failed("llm::extract", format!("BAML error: {e}"))
                })?;

                Ok(json!({
                    "label": result.label,
                    "confidence": result.confidence,
                    "reasoning": result.reasoning,
                }))
            }
            unknown => Err(CruxErr::step_failed(
                "llm::extract",
                format!(
                    "unknown BAML function '{unknown}'; expected one of: \
                     ExtractEntities, Summarize, Classify"
                ),
            )),
        }
    });
}

/// Register the `llm::decompose` handler. Only compiled when the `baml` feature is enabled.
#[cfg(feature = "baml")]
pub fn register_decompose(registry: &mut HandlerRegistry) {
    use crate::baml_client::async_client::B;

    registry.handler("llm::decompose", |input: Value| async move {
        let text = input
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CruxErr::step_failed("llm::decompose", "missing 'text' field"))?
            .to_string();

        let result = B
            .DecomposeSpec
            .call(text)
            .await
            .map_err(|e| CruxErr::step_failed("llm::decompose", format!("BAML error: {e}")))?;

        let tasks: Vec<Value> = result
            .tasks
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "name": t.name,
                    "title": t.title,
                    "description": t.description,
                    "priority": t.priority,
                    "status": t.status,
                    "files": t.files,
                })
            })
            .collect();

        Ok(serde_json::json!({ "tasks": tasks }))
    });
}

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler("llm::invoke", |input: Value| async move {
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
        if let Some(meta) = resp.metadata {
            if let (Some(map), Some(meta_obj)) = (out.as_object_mut(), meta.as_object()) {
                for (k, v) in meta_obj {
                    map.insert(k.clone(), v.clone());
                }
            }
        }
        Ok(out)
    });
}
