use crux_script::{HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;

/// When the current pipeline input is not a JSON object, the runner wraps it under the `"input"`
/// key so that static `args` can still be injected cleanly without clobbering the payload.
#[tokio::test]
async fn step_args_wraps_non_object_input() {
    let yaml = r#"
pipeline: test_args_scalar
steps:
  - step: run_cmd
    handler: echo_both
    args:
      cmd: "ls"
"#;

    let pipeline = load(yaml).unwrap();
    let mut registry = HandlerRegistry::new();
    registry.handler_value("echo_both", |input: Value| async move {
        // Non-object inputs are wrapped: { "args": { "cmd": "ls" }, "input": <original> }
        let cmd = input["args"]["cmd"].as_str().unwrap_or("").to_string();
        let original = input["input"].clone();
        Ok(json!({ "cmd": cmd, "original": original }))
    });

    let runner = Runner::new(Arc::new(registry));
    // Pass a scalar string as input — not a JSON object
    let crux = runner.run(&pipeline, json!("raw-scalar")).await;
    assert!(
        crux.value().is_ok(),
        "run should succeed: {:?}",
        crux.value()
    );
    let out = crux.value().unwrap();
    assert_eq!(out["cmd"].as_str().unwrap(), "ls");
    assert_eq!(out["original"].as_str().unwrap(), "raw-scalar");
}

#[tokio::test]
async fn step_args_merged_into_handler_input() {
    let yaml = r#"
pipeline: test_args
steps:
  - step: run_cmd
    handler: echo_args
    args:
      cmd: "echo hello"
      cwd: "/tmp"
"#;

    let pipeline = load(yaml).unwrap();
    let mut registry = HandlerRegistry::new();
    registry.handler_value("echo_args", |input: Value| async move {
        // Should receive { "args": { "cmd": "echo hello", "cwd": "/tmp" } }
        let cmd = input["args"]["cmd"].as_str().unwrap_or("").to_string();
        Ok(json!({ "received_cmd": cmd }))
    });

    let runner = Runner::new(Arc::new(registry));
    let crux = runner.run(&pipeline, json!(null)).await;
    assert!(crux.value().is_ok());
    let out = crux.value().unwrap();
    assert_eq!(out["received_cmd"].as_str().unwrap(), "echo hello");
}
