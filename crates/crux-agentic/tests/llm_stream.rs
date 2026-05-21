/// Verify that `llm::stream` handler is registered and returns a result.
/// The current implementation is a stub that buffers the full response;
/// real streaming requires async-stream trait extension (tracked in #21).
use crux_script::HandlerRegistry;
use serde_json::json;

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
    let result = handler(json!({})).await;
    assert!(result.is_err(), "missing prompt should return error");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("prompt"),
        "error should mention 'prompt', got: {msg}"
    );
}
