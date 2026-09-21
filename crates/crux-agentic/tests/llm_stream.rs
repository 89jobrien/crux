use crux_script::HandlerRegistry;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn llm_stream_handler_is_registered() {
    let mut reg = HandlerRegistry::new();
    crux_agentic::register_all(&mut reg);
    assert!(
        reg.get_handler("llm::stream").is_some(),
        "llm::stream must be registered by register_all"
    );
}

#[tokio::test]
async fn llm_stream_missing_prompt_returns_error() {
    let mut reg = HandlerRegistry::new();
    crux_agentic::register_all(&mut reg);
    let handler = reg.get_handler("llm::stream").unwrap();
    let result = handler(json!({})).await.outcome;
    assert!(result.is_err(), "missing prompt should return error");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("prompt"),
        "error should mention 'prompt', got: {msg}"
    );
}

#[tokio::test]
async fn llm_stream_collects_incremental_openai_deltas() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 4096];
        let read = stream.read(&mut request).await.unwrap();
        let request = String::from_utf8_lossy(&request[..read]);
        assert!(request.contains(r#""stream":true"#));

        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        stream
            .write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n")
            .await
            .unwrap();
        stream.flush().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        stream
            .write_all(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\ndata: [DONE]\n\n",
            )
            .await
            .unwrap();
    });

    let mut registry = HandlerRegistry::new();
    crux_agentic::llm::register_stream(&mut registry);
    let handler = registry.get_handler("llm::stream").unwrap();
    let output = handler(json!({
        "prompt": "hello",
        "args": {
            "provider": "openai",
            "model": "test-model",
            "base_url": format!("http://{address}")
        }
    }))
    .await
    .outcome
    .unwrap()
    .value;
    server.await.unwrap();

    assert_eq!(output["content"], "Hello");
    assert_eq!(output["chunks"], json!(["Hel", "lo"]));
    assert_eq!(output["streaming"], true);
}
