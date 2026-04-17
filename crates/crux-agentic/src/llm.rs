use crate::error::opt_str;
use cruxai_core::prelude::CruxErr;
use cruxai_script::HandlerRegistry;
use serde_json::{json, Value};

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
