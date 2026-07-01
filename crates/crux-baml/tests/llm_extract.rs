// Integration tests for llm::extract handler using MockLLM — no API keys required.

mod mock_baml;

use crate::mock_baml::{MockBamlServer, default_responses};
use crux_baml::register_extract_with;
use crux_script::{HandlerRegistry, handler_output::HandlerOutput};
use serde_json::json;

async fn make_mock_registry() -> (MockBamlServer, HandlerRegistry) {
    let server = MockBamlServer::start(default_responses()).await;
    let client_registry = server.registry();
    let mut registry = HandlerRegistry::new();
    register_extract_with(&mut registry, Some(client_registry));
    (server, registry)
}

async fn invoke(
    registry: &HandlerRegistry,
    input: serde_json::Value,
) -> Result<HandlerOutput, Box<dyn std::error::Error + Send + Sync>> {
    let handler = registry
        .get_handler("llm::extract")
        .expect("llm::extract handler must be registered");
    Ok(handler(input).await?)
}

#[tokio::test]
async fn extract_entities_returns_structured_output() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "function": "ExtractEntities",
        "input": {
            "text": "Elon Musk founded SpaceX in Hawthorne, California in 2002."
        }
    });

    let result = invoke(&registry, input)
        .await
        .expect("llm::extract should succeed");

    let entities = result
        .get("entities")
        .and_then(|v| v.as_array())
        .expect("result must contain 'entities' array");

    assert!(!entities.is_empty(), "expected at least one entity");

    for entity in entities {
        assert!(entity.get("name").is_some(), "entity must have 'name'");
        assert!(
            entity.get("entity_type").is_some(),
            "entity must have 'entity_type'"
        );
    }
}

#[tokio::test]
async fn summarize_returns_structured_output() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "function": "Summarize",
        "input": {
            "text": "Rust is a systems programming language focused on three goals: \
                     safety, speed, and concurrency. It accomplishes these goals without \
                     a garbage collector, making it useful for a number of use cases other \
                     languages are not good at: embedding in other languages, programs with \
                     specific space and time requirements, and writing low-level code, like \
                     device drivers and operating systems.",
            "max_sentences": 2
        }
    });

    let result = invoke(&registry, input)
        .await
        .expect("llm::extract should succeed");

    assert!(
        result.get("summary").is_some(),
        "result must contain 'summary'"
    );
    assert!(
        result.get("key_points").is_some(),
        "result must contain 'key_points'"
    );
}

#[tokio::test]
async fn classify_returns_structured_output() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "function": "Classify",
        "input": {
            "text": "I absolutely love this product! It exceeded all my expectations.",
            "labels": ["positive", "negative", "neutral"]
        }
    });

    let result = invoke(&registry, input)
        .await
        .expect("llm::extract should succeed");

    let label = result
        .get("label")
        .and_then(|v| v.as_str())
        .expect("result must contain 'label' string");

    assert_eq!(label, "positive", "sentiment should be positive");

    let confidence = result
        .get("confidence")
        .and_then(|v| v.as_f64())
        .expect("result must contain 'confidence' float");

    assert!(
        (0.0..=1.0).contains(&confidence),
        "confidence must be between 0.0 and 1.0"
    );
}

/// ClassifyCIFailure wiring test — verifies the handler recognises the function.
#[tokio::test]
async fn classify_ci_failure_is_wired() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "function": "ClassifyCIFailure",
        "input": {
            "failure_output": "error[E0502]: borrow checker error",
            "known_patterns": []
        }
    });

    let handler = registry
        .get_handler("llm::extract")
        .expect("llm::extract handler must be registered");

    let result = handler(input).await;
    if let Err(e) = result {
        let msg = e.to_string();
        assert!(
            !msg.contains("unknown BAML function"),
            "ClassifyCIFailure should be wired, got: {msg}"
        );
    }
}

#[tokio::test]
async fn unknown_function_returns_error() {
    let (_server, registry) = make_mock_registry().await;
    let input = json!({
        "function": "NonExistentFunction",
        "input": { "text": "hello" }
    });

    let handler = registry
        .get_handler("llm::extract")
        .expect("llm::extract handler must be registered");

    let err = handler(input)
        .await
        .expect_err("unknown function should return an error");

    let msg = err.to_string();
    assert!(
        msg.contains("NonExistentFunction"),
        "error should mention the unknown function name, got: {msg}"
    );
}
