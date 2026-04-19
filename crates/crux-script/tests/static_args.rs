use cruxai_script::{HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;

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
    registry.handler("echo_args", |input: Value| async move {
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
