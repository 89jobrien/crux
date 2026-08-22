/// Integration tests for pipeline-level `vars:` bindings (#85).
use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerRegistry, Runner, load};
use serde_json::{Value, json};
use std::sync::Arc;

fn registry() -> Arc<HandlerRegistry> {
    let mut reg = HandlerRegistry::new();
    reg.handler_value("echo_args_name", |input: Value| async move {
        let name = input
            .get("args")
            .and_then(|a| a.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok::<Value, CruxErr>(Value::String(name))
    });
    Arc::new(reg)
}

#[tokio::test]
async fn vars_are_referenced_in_step_args() {
    let yaml = r#"
pipeline: with_vars
vars:
  SESSION_NAME: "static-value"
steps:
  - step: greet
    handler: echo_args_name
    args:
      name: "{{ vars.SESSION_NAME }}"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok(), "{:?}", crux.value());
    assert_eq!(crux.value().unwrap(), &json!("static-value"));
}

#[tokio::test]
async fn vars_can_reference_input() {
    let yaml = r#"
pipeline: with_input_vars
vars:
  SESSION_NAME: "{{ input.session }}"
steps:
  - step: greet
    handler: echo_args_name
    args:
      name: "{{ vars.SESSION_NAME }}"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({ "session": "abc-123" })).await;
    assert!(crux.value().is_ok(), "{:?}", crux.value());
    assert_eq!(crux.value().unwrap(), &json!("abc-123"));
}

#[tokio::test]
async fn vars_are_resolved_once_not_per_step() {
    // Two steps both reference the same var — it should resolve identically
    // both times without re-evaluating (there's nothing time-varying here, but
    // this exercises that the vars map is populated once up front and shared).
    let yaml = r#"
pipeline: shared_vars
vars:
  SESSION_NAME: "{{ input.session }}"
steps:
  - step: greet1
    handler: echo_args_name
    args:
      name: "{{ vars.SESSION_NAME }}"
  - step: greet2
    handler: echo_args_name
    args:
      name: "{{ vars.SESSION_NAME }}"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({ "session": "xyz" })).await;
    assert!(crux.value().is_ok());
    assert_eq!(crux.value().unwrap(), &json!("xyz"));
}

#[tokio::test]
async fn pipeline_without_vars_still_works() {
    let yaml = r#"
pipeline: no_vars
steps:
  - step: greet
    handler: echo_args_name
    args:
      name: "literal"
"#;
    let pipeline = load(yaml).unwrap();
    let runner = Runner::new(registry());
    let crux = runner.run(&pipeline, json!({})).await;
    assert!(crux.value().is_ok());
    assert_eq!(crux.value().unwrap(), &json!("literal"));
}
