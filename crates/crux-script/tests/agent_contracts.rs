//! Compilation tests for delegated-agent input and output contracts.

use crux_script::{AgentMetadata, HandlerRegistry, RegistryError, ValueSchema};
use serde_json::{Value, json};

fn metadata(name: &str) -> AgentMetadata {
    AgentMetadata::new(name)
        .input_schema(ValueSchema::Dynamic)
        .output_schema(ValueSchema::String)
}

#[test]
fn agent_registration_requires_unique_names() {
    let mut registry = HandlerRegistry::new();
    registry
        .agent_fn_with_metadata(
            metadata("test-agent"),
            |input: Value| async move { Ok(input) },
        )
        .unwrap();

    assert_eq!(
        registry.agent_fn_with_metadata(
            metadata("test-agent"),
            |input: Value| async move { Ok(input) }
        ),
        Err(RegistryError::DuplicateAgent {
            name: "test-agent".to_string(),
        })
    );
}

#[test]
fn agent_contract_completeness() {
    assert!(!AgentMetadata::new("dynamic-agent").has_complete_contract());
    assert!(metadata("typed-agent").has_complete_contract());
}

#[tokio::test]
async fn agent_binding_is_resolvable() {
    let mut registry = HandlerRegistry::new();
    registry
        .agent_fn_with_metadata(
            metadata("echo-agent"),
            |input: Value| async move { Ok(input) },
        )
        .unwrap();

    assert_eq!(
        registry.agent_metadata("echo-agent").unwrap().name,
        "echo-agent"
    );
    let output = registry.get_agent("echo-agent").unwrap()(json!("hello"))
        .await
        .unwrap();
    assert_eq!(output, json!("hello"));
}
