use crux_agentic::http;
use crux_script::{Capability, HandlerRegistry, RiskLevel, SideEffect};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn registry() -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();
    http::register(&mut registry);
    registry
}

async fn mock_server(
    status: &str,
    body: &'static str,
) -> (String, tokio::sync::oneshot::Receiver<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (request_tx, request_rx) = tokio::sync::oneshot::channel();
    let status = status.to_owned();

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 4096];
        let read = stream.read(&mut request).await.unwrap();
        request.truncate(read);
        request_tx
            .send(String::from_utf8(request).unwrap())
            .unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    });

    (format!("http://{address}"), request_rx)
}

fn local_policy() -> serde_json::Value {
    json!({
        "allowed_hosts": ["127.0.0.1"],
        "allow_http": true,
        "allow_private_networks": true
    })
}

#[test]
fn http_handler_declares_network_capability_metadata() {
    let registry = registry();
    let metadata = registry.get_metadata(http::HTTP_REQUEST).unwrap();

    assert_eq!(metadata.risk, RiskLevel::Medium);
    assert_eq!(metadata.side_effects, vec![SideEffect::Network]);
    assert_eq!(metadata.capabilities, vec![Capability::Network]);
    assert!(!metadata.deterministic);
}

#[tokio::test]
async fn http_handler_enforces_allowed_hosts() {
    let registry = registry();
    let handler = registry.get_handler(http::HTTP_REQUEST).unwrap();
    let outcome = handler(json!({
        "args": {
            "method": "GET",
            "url": "https://example.com/data",
            "network_policy": {"allowed_hosts": ["api.example.com"]}
        }
    }))
    .await
    .outcome;

    let error = outcome.unwrap_err().to_string();
    assert!(error.contains("not allowed"), "unexpected error: {error}");
}

#[tokio::test]
async fn http_handler_sends_typed_request_and_returns_bounded_response() {
    let (url, request_rx) = mock_server("201 Created", "hello").await;
    let registry = registry();
    let handler = registry.get_handler(http::HTTP_REQUEST).unwrap();
    let output = handler(json!({
        "args": {
            "method": "POST",
            "url": url,
            "headers": {"x-crux-test": "typed"},
            "body": {"message": "safe"},
            "max_response_bytes": 32,
            "timeout_ms": 1000,
            "network_policy": local_policy()
        }
    }))
    .await
    .outcome
    .unwrap();

    assert_eq!(output["status"], 201);
    assert_eq!(output["body"], "hello");
    let request = request_rx.await.unwrap().to_ascii_lowercase();
    assert!(request.starts_with("post / http/1.1"));
    assert!(request.contains("x-crux-test: typed"));
    assert!(request.contains(r#"{"message":"safe"}"#));
}

#[tokio::test]
async fn http_handler_rejects_responses_over_limit() {
    let (url, _request_rx) = mock_server("200 OK", "response-too-large").await;
    let registry = registry();
    let handler = registry.get_handler(http::HTTP_REQUEST).unwrap();
    let outcome = handler(json!({
        "args": {
            "method": "GET",
            "url": url,
            "max_response_bytes": 4,
            "timeout_ms": 1000,
            "network_policy": local_policy()
        }
    }))
    .await
    .outcome;

    let error = outcome.unwrap_err().to_string();
    assert!(
        error.contains("response limit"),
        "unexpected error: {error}"
    );
}
