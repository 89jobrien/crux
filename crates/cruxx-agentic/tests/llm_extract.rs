// Integration tests for llm::extract handler.
//
// These tests make real API calls and are skipped when OPENAI_API_KEY is not set.
// Run with: cargo nextest run --features baml -p cruxx-agentic

#![cfg(feature = "baml")]

use cruxx_agentic::llm::register_extract;
use cruxx_script::HandlerRegistry;
use serde_json::json;

fn make_registry() -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();
    register_extract(&mut registry);
    registry
}

async fn invoke(
    registry: &HandlerRegistry,
    input: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let handler = registry
        .get_handler("llm::extract")
        .expect("llm::extract handler must be registered");
    Ok(handler(input).await?)
}

/// Returns true when an OpenAI API key is available in the environment.
fn has_api_key() -> bool {
    std::env::var("OPENAI_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false)
}

#[tokio::test]
async fn extract_entities_returns_structured_output() {
    if !has_api_key() {
        eprintln!("OPENAI_API_KEY not set — skipping llm::extract integration test");
        return;
    }

    let registry = make_registry();
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
    if !has_api_key() {
        eprintln!("OPENAI_API_KEY not set — skipping llm::extract integration test");
        return;
    }

    let registry = make_registry();
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
    if !has_api_key() {
        eprintln!("OPENAI_API_KEY not set — skipping llm::extract integration test");
        return;
    }

    let registry = make_registry();
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

#[tokio::test]
async fn unknown_function_returns_error() {
    let registry = make_registry();
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
