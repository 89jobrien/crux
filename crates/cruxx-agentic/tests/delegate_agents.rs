/// Verify that agents can be pre-registered for delegate: steps.
use cruxx_script::HandlerRegistry;
use serde_json::{Value, json};

#[tokio::test]
async fn register_agent_fn_allows_delegate_lookup() {
    let mut reg = HandlerRegistry::new();

    // Register a named agent via the closure-based API.
    reg.agent_fn("my-agent", |input: Value| async move {
        Ok(json!({ "echoed": input }))
    });

    // The agent must be findable by name.
    let runner = reg.get_agent("my-agent");
    assert!(runner.is_some(), "agent 'my-agent' should be registered");

    // And must execute correctly.
    let result = runner.unwrap()(json!({ "x": 1 })).await.unwrap();
    assert_eq!(result["echoed"]["x"], 1);
}

#[tokio::test]
async fn register_all_with_agents_wires_custom_agents() {
    let mut reg = HandlerRegistry::new();
    cruxx_agentic::register_all(&mut reg);

    // register_agents allows adding user-defined agents after register_all.
    cruxx_agentic::register_agent(
        &mut reg,
        "test-pipeline-agent",
        |_input: Value| async move { Ok(json!({ "status": "ok" })) },
    );

    let runner = reg.get_agent("test-pipeline-agent");
    assert!(runner.is_some(), "test-pipeline-agent should be registered");
}
