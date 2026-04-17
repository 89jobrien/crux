use crate::error::opt_str;
use cruxai_core::prelude::CruxErr;
use cruxai_script::HandlerRegistry;
use serde_json::{json, Value};

/// Register the `llm::extract` handler.  Only compiled when the `baml` feature is enabled.
#[cfg(feature = "baml")]
pub fn register_extract(registry: &mut HandlerRegistry) {
    use crate::baml_client::async_client::B;

    registry.handler("llm::extract", |input: Value| async move {
        let function = input
            .get("function")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CruxErr::step_failed("llm::extract", "missing 'function' field")
            })?
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

                let result = b
                    .ExtractEntities
                    .call(text)
                    .await
                    .map_err(|e| {
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

                let result = b
                    .Summarize
                    .call(text, max_sentences)
                    .await
                    .map_err(|e| {
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

                let result = b
                    .Classify
                    .call(text, &labels)
                    .await
                    .map_err(|e| {
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

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler("llm::complete", |input: Value| async move {
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CruxErr::step_failed("llm::complete", "missing 'prompt' field"))?
            .to_string();

        let provider = opt_str(&input, "provider").unwrap_or("openai").to_string();
        let model = opt_str(&input, "model")
            .unwrap_or("gpt-4o-mini")
            .to_string();
        let system = opt_str(&input, "system")
            .unwrap_or("You are a helpful assistant.")
            .to_string();
        let max_tokens = input
            .get("args")
            .and_then(|a| a.get("max_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1024);
        let api_key = opt_str(&input, "api_key")
            .map(str::to_string)
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .unwrap_or_default();

        match provider.as_str() {
            "anthropic" => {
                let base_url = opt_str(&input, "base_url")
                    .unwrap_or("https://api.anthropic.com")
                    .to_string();
                complete_anthropic(&base_url, &model, &system, &prompt, max_tokens, &api_key).await
            }
            _ => {
                let base_url = opt_str(&input, "base_url")
                    .unwrap_or("https://api.openai.com")
                    .to_string();
                complete_openai(&base_url, &model, &system, &prompt, max_tokens, &api_key).await
            }
        }
    });
}

async fn complete_openai(
    base_url: &str,
    model: &str,
    system: &str,
    prompt: &str,
    max_tokens: u64,
    api_key: &str,
) -> Result<Value, CruxErr> {
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": prompt}
        ]
    });

    let client = reqwest::Client::new();
    let mut req = client.post(&url).json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }

    let resp = req.send().await.map_err(|e| {
        CruxErr::step_failed("llm::complete", format!("HTTP error: {e}"))
    })?;

    let json: Value = resp.json().await.map_err(|e| {
        CruxErr::step_failed("llm::complete", format!("JSON decode error: {e}"))
    })?;

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| {
            CruxErr::step_failed("llm::complete", "unexpected response shape")
        })?
        .to_string();

    Ok(json!({
        "content": content,
        "model": json["model"],
        "usage": json["usage"],
    }))
}

async fn complete_anthropic(
    base_url: &str,
    model: &str,
    system: &str,
    prompt: &str,
    max_tokens: u64,
    api_key: &str,
) -> Result<Value, CruxErr> {
    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "system": system,
        "messages": [
            {"role": "user", "content": prompt}
        ]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .map_err(|e| CruxErr::step_failed("llm::complete", format!("HTTP error: {e}")))?;

    let json: Value = resp.json().await.map_err(|e| {
        CruxErr::step_failed("llm::complete", format!("JSON decode error: {e}"))
    })?;

    let content = json["content"][0]["text"]
        .as_str()
        .ok_or_else(|| {
            CruxErr::step_failed("llm::complete", "unexpected response shape")
        })?
        .to_string();

    Ok(json!({
        "content": content,
        "model": json["model"],
        "usage": json["usage"],
    }))
}
