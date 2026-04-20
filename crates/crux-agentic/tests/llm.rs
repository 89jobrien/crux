use crux_agentic::llm;
use cruxai_script::HandlerRegistry;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    llm::register(&mut r);
    r
}

async fn mock_openai_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        // Drain the incoming request; exact byte count doesn't matter for mock purposes.
        let mut buf = vec![0u8; 4096];
        let _n = stream.read(&mut buf).await.unwrap();

        let body = r#"{"id":"test","choices":[{"message":{"content":"4","role":"assistant"}}],"model":"test-model","usage":{"prompt_tokens":5,"completion_tokens":1,"total_tokens":6}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    });

    (format!("http://127.0.0.1:{port}"), handle)
}

async fn mock_anthropic_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        // Drain the incoming request; exact byte count doesn't matter for mock purposes.
        let mut buf = vec![0u8; 4096];
        let _n = stream.read(&mut buf).await.unwrap();

        let body = r#"{"id":"msg_test","content":[{"type":"text","text":"4"}],"model":"claude-test","usage":{"input_tokens":5,"output_tokens":1}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    });

    (format!("http://127.0.0.1:{port}"), handle)
}

#[tokio::test]
async fn complete_openai_compat() {
    let (base_url, _server) = mock_openai_server().await;
    let reg = registry();
    let handler = reg.get_handler("llm::invoke").unwrap();
    let result = handler(json!({
        "prompt": "What is 2+2?",
        "args": {
            "provider": "openai",
            "base_url": base_url,
            "model": "test-model"
        }
    }))
    .await
    .unwrap();

    assert_eq!(result["content"].as_str().unwrap(), "4");
    assert!(result["usage"].is_object());
}

#[tokio::test]
async fn complete_anthropic_path() {
    let (base_url, _server) = mock_anthropic_server().await;
    let reg = registry();
    let handler = reg.get_handler("llm::invoke").unwrap();
    let result = handler(json!({
        "prompt": "What is 2+2?",
        "args": {
            "provider": "anthropic",
            "base_url": base_url,
            "model": "claude-test",
            "max_tokens": 10
        }
    }))
    .await
    .unwrap();

    assert_eq!(result["content"].as_str().unwrap(), "4");
    assert!(result["usage"].is_object());
}

#[tokio::test]
async fn complete_missing_prompt_errors() {
    let reg = registry();
    let handler = reg.get_handler("llm::invoke").unwrap();
    let result = handler(json!({"args": {"model": "x"}})).await;
    assert!(result.is_err());
}
