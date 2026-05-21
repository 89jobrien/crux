#![cfg(feature = "baml")]

//! Mock OpenAI-compatible server for BAML integration tests.
//!
//! Serves canned responses keyed by BAML function name (extracted from the
//! prompt). Tests create a `ClientRegistry` pointing at the mock server,
//! then call BAML functions via `B.FunctionName.with_client_registry(&reg)`.

use serde_json::json;
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// Canned response bodies keyed by a substring match on the request body.
/// The mock server scans the POST body for each key and returns the first
/// matching response as the `content` field of a chat completion.
pub struct MockBamlServer {
    pub base_url: String,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockBamlServer {
    /// Start a mock server that routes requests based on function-name
    /// substrings in the request body.
    pub async fn start(responses: HashMap<&'static str, String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base_url = format!("http://127.0.0.1:{port}/v1");

        let handle = tokio::spawn(async move {
            // Serve multiple requests (BAML may retry or make multiple calls).
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let responses = responses.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stream);

                    // Read HTTP headers to find Content-Length.
                    let mut headers = String::new();
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).await.unwrap() == 0 {
                            return;
                        }
                        headers.push_str(&line);
                        if line == "\r\n" {
                            break;
                        }
                    }

                    let content_length = headers
                        .lines()
                        .find_map(|l| {
                            let lower = l.to_lowercase();
                            if lower.starts_with("content-length:") {
                                lower
                                    .trim_start_matches("content-length:")
                                    .trim()
                                    .parse::<usize>()
                                    .ok()
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);

                    let mut body_buf = vec![0u8; content_length];
                    reader.read_exact(&mut body_buf).await.unwrap();
                    let body = String::from_utf8_lossy(&body_buf);

                    // Find matching response.
                    let content = responses
                        .iter()
                        .find(|(key, _)| body.contains(*key))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_else(|| r#"{"error": "no mock match"}"#.to_string());

                    let resp_body = json!({
                        "id": "mock-001",
                        "object": "chat.completion",
                        "choices": [{
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": content
                            },
                            "finish_reason": "stop"
                        }],
                        "model": "mock-model",
                        "usage": {
                            "prompt_tokens": 10,
                            "completion_tokens": 5,
                            "total_tokens": 15
                        }
                    })
                    .to_string();

                    let http_resp = format!(
                        "HTTP/1.1 200 OK\r\n\
                         Content-Type: application/json\r\n\
                         Content-Length: {}\r\n\
                         \r\n\
                         {}",
                        resp_body.len(),
                        resp_body
                    );

                    let stream = reader.into_inner();
                    let mut stream = stream;
                    let _ = stream.write_all(http_resp.as_bytes()).await;
                });
            }
        });

        Self {
            base_url,
            _handle: handle,
        }
    }

    /// Build a `ClientRegistry` that routes all BAML calls to this mock.
    pub fn registry(&self) -> baml::ClientRegistry {
        let mut reg = baml::ClientRegistry::new();
        let mut opts = HashMap::new();
        opts.insert("model".to_string(), json!("mock-model"));
        opts.insert("api_key".to_string(), json!("mock-key"));
        opts.insert("base_url".to_string(), json!(&self.base_url));
        reg.add_llm_client("MockLLM", "openai", opts);
        reg.set_primary_client("MockLLM");
        reg
    }
}

/// Canned BAML-parseable responses for each function.
///
/// Keys use unique schema fragments from each function's `ctx.output_format`
/// to avoid false matches (e.g. "Summarize" appearing in GeneratePipeline's
/// prompt). The match order matters — more specific keys are checked first.
pub fn default_responses() -> HashMap<&'static str, String> {
    let mut m = HashMap::new();

    // Use unique schema fragments as match keys — these come from BAML's
    // generated `ctx.output_format` and are unique per function.

    m.insert(
        "entity_type: string",
        json!({
            "entities": [
                {"name": "Rust", "entity_type": "CONCEPT", "description": "programming language"},
                {"name": "Mozilla", "entity_type": "ORGANIZATION", "description": null}
            ]
        })
        .to_string(),
    );

    m.insert(
        "word_count: int",
        json!({
            "summary": "A concise summary of the input text.",
            "key_points": ["point one", "point two"],
            "word_count": 42
        })
        .to_string(),
    );

    m.insert(
        "fix_type: string",
        json!({
            "kind": "false-positive",
            "fix_type": "obfsck-ignore",
            "suggested_fix": "Add pattern to allowlist",
            "confidence": 0.95,
            "new_pattern": "test fixture values"
        })
        .to_string(),
    );

    m.insert(
        "reasoning: string or null",
        json!({
            "label": "positive",
            "confidence": 0.92,
            "reasoning": "Strong positive sentiment"
        })
        .to_string(),
    );

    m.insert(
        "class DescribeProjectOutput",
        json!({
            "description": "Agentic DSL runtime for Rust pipelines"
        })
        .to_string(),
    );

    m.insert(
        "dormant",
        json!({
            "status": "active",
            "confidence": 0.88,
            "reason": "Regular commits in the last 30 days"
        })
        .to_string(),
    );

    m.insert(
        "category: string",
        json!({
            "category": "library",
            "confidence": 0.91
        })
        .to_string(),
    );

    m.insert(
        "highlights: string[]",
        json!({
            "summary": "Added mock testing support",
            "highlights": ["Mock BAML server", "CI-safe tests"]
        })
        .to_string(),
    );

    m.insert(
        "related: string[]",
        json!({
            "related": ["minibox", "devkit", "braid"]
        })
        .to_string(),
    );

    m.insert(
        "files: string[]",
        json!({
            "tasks": [{
                "id": "add-mock",
                "name": "add_mock",
                "title": "Add mock BAML server",
                "description": "Create mock server for testing",
                "priority": "P1",
                "status": "open",
                "files": ["tests/mock_baml.rs"]
            }]
        })
        .to_string(),
    );

    m.insert(
        "delegate_handler: string",
        json!({
            "pipeline": "test-pipeline",
            "budget": null,
            "steps": [{
                "step": {"step": "read", "handler": "fs::read", "args": null},
                "delegation": null,
                "pipe": null,
                "join_all": null,
                "route": null,
                "speculate": null
            }]
        })
        .to_string(),
    );

    m
}
