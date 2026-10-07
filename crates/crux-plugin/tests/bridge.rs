//! Subprocess integration tests for bridging plugin handlers into a registry.

use crux_plugin::bridge::register_plugins;
use crux_plugin::manifest::PluginEntry;
use crux_script::{
    CompileOptions, HandlerRegistry, Runner, ValidationCode, ValueSchema, compile_pipeline, load,
};
use std::collections::HashMap;
use std::sync::Arc;

fn echo_entry() -> PluginEntry {
    let bin = env!("CARGO_BIN_EXE_echo-plugin");
    PluginEntry {
        name: "echo".into(),
        path: bin.into(),
        env: HashMap::new(),
    }
}

/// An echo plugin whose `handler` declares `schema` as its result shape.
///
/// The schema is serialized from the real `ValueSchema` type rather than
/// hand-written JSON, so these tests cannot drift from the wire format.
fn typed_echo_entry(plugin: &str, handler: &str, schema: ValueSchema) -> PluginEntry {
    let mut entry = echo_entry();
    entry.name = plugin.into();
    entry.env.insert("ECHO_HANDLER".into(), handler.into());
    entry.env.insert(
        "ECHO_OUTPUT_SCHEMA".into(),
        serde_json::to_string(&schema).unwrap(),
    );
    entry
}

fn string_array() -> ValueSchema {
    ValueSchema::array(ValueSchema::String)
}

fn has_dynamic_boundary<T>(result: &crux_script::Compilation<T>) -> bool {
    result
        .diagnostics()
        .iter()
        .any(|d| d.code == ValidationCode::DynamicBoundary)
}

fn has_missing_contract<T>(result: &crux_script::Compilation<T>) -> bool {
    result
        .diagnostics()
        .iter()
        .any(|d| d.code == ValidationCode::MissingContract)
}

/// A pipeline that iterates a plugin step's output, so the compiler has to
/// know that output is an array.
fn for_each_pipeline(handler: &str, binding: &str) -> crux_script::schema::PipelineDef {
    load(&format!(
        r#"
pipeline: plugin-for-each
input_schema:
  type: string
steps:
  - step: fetch
    handler: {handler}
  - for_each: "as {binding}"
    items: "{{{{ steps.fetch.output }}}}"
    steps:
      - step: absorb
        handler: sink::absorb
"#
    ))
    .unwrap()
}

fn delayed_echo_entry(name: &str, handler: &str) -> PluginEntry {
    let mut entry = echo_entry();
    entry.name = name.into();
    entry.env.insert("ECHO_HANDLER".into(), handler.into());
    entry.env.insert("ECHO_DELAY_MS".into(), "250".into());
    entry
}

#[tokio::test]
async fn bridge_registers_plugin_handlers_in_registry() {
    let mut registry = HandlerRegistry::new();
    let entries = vec![echo_entry()];
    register_plugins(&mut registry, &entries).await.unwrap();
    assert!(
        registry.get_handler("echo::reflect").is_some(),
        "echo::reflect should be registered"
    );
}

#[tokio::test]
async fn bridge_handler_invokes_plugin() {
    let mut registry = HandlerRegistry::new();
    let entries = vec![echo_entry()];
    register_plugins(&mut registry, &entries).await.unwrap();

    let handler = registry.get_handler("echo::reflect").unwrap().clone();
    let input = serde_json::json!({"data": "test"});
    let output = handler(input.clone()).await.outcome.unwrap().value;
    assert_eq!(output, input);
}

#[tokio::test]
async fn handlers_from_independent_plugins_run_concurrently() {
    let mut registry = HandlerRegistry::new();
    let entries = vec![
        delayed_echo_entry("first", "echo::first"),
        delayed_echo_entry("second", "echo::second"),
    ];
    register_plugins(&mut registry, &entries).await.unwrap();

    let first = registry.get_handler("echo::first").unwrap().clone();
    let second = registry.get_handler("echo::second").unwrap().clone();
    let started = tokio::time::Instant::now();
    let (first_result, second_result) = tokio::join!(
        first(serde_json::json!({"plugin": "first"})),
        second(serde_json::json!({"plugin": "second"})),
    );

    assert_eq!(
        first_result.outcome.unwrap().value,
        serde_json::json!({"plugin": "first"})
    );
    assert_eq!(
        second_result.outcome.unwrap().value,
        serde_json::json!({"plugin": "second"})
    );
    assert!(
        started.elapsed() < std::time::Duration::from_millis(425),
        "independent plugin processes were serialized"
    );
}

#[tokio::test]
async fn declared_output_schema_reaches_handler_metadata() {
    let mut registry = HandlerRegistry::new();
    let entries = vec![typed_echo_entry("typed", "echo::typed", string_array())];
    register_plugins(&mut registry, &entries).await.unwrap();

    let metadata = registry.get_metadata("echo::typed").unwrap();
    assert_eq!(metadata.output_schema, Some(string_array()));
    assert_eq!(metadata.description, "Returns input unchanged");
}

#[tokio::test]
async fn undeclared_output_schema_stays_dynamic() {
    let mut registry = HandlerRegistry::new();
    register_plugins(&mut registry, &[echo_entry()])
        .await
        .unwrap();

    let metadata = registry.get_metadata("echo::reflect").unwrap();
    assert_eq!(
        metadata.output_schema,
        Some(ValueSchema::Dynamic),
        "a plugin that declares nothing should keep the pre-existing Dynamic contract"
    );
}

#[tokio::test]
async fn a_malformed_declared_schema_fails_at_load_time() {
    let mut registry = HandlerRegistry::new();
    // Deserializes cleanly, but an empty union is not a valid schema.
    let entries = vec![typed_echo_entry(
        "typed",
        "echo::typed",
        ValueSchema::Union { variants: vec![] },
    )];

    let error = register_plugins(&mut registry, &entries)
        .await
        .expect_err("an empty union must not reach the registry");

    assert!(
        matches!(error, crux_plugin::host::PluginError::InvalidSchema { .. }),
        "expected InvalidSchema, got {error:?}"
    );
}

/// The payoff for #127: a `for_each` over a plugin step is only type-checkable
/// once the plugin declares its output shape. Under the old registration path
/// the step was `Dynamic` and the compiler could not prove it was an array.
#[tokio::test]
async fn a_typed_plugin_output_resolves_the_for_each_dynamic_boundary() {
    let mut registry = HandlerRegistry::new();
    registry.handler_value("sink::absorb", |input| async move { Ok(input) });
    let entries = vec![typed_echo_entry("typed", "echo::typed", string_array())];
    register_plugins(&mut registry, &entries).await.unwrap();

    let pipeline = for_each_pipeline("echo::typed", "line");
    let result = compile_pipeline(&pipeline, &registry, CompileOptions::permissive());

    assert!(
        !has_dynamic_boundary(&result),
        "a declared array output should resolve the for_each boundary, got {:?}",
        result.diagnostics()
    );
}

/// The same pipeline against a plugin that declares nothing must still report
/// the boundary, so the fix types plugin outputs rather than silencing the
/// check.
#[tokio::test]
async fn an_untyped_plugin_output_still_reports_the_dynamic_boundary() {
    let mut registry = HandlerRegistry::new();
    registry.handler_value("sink::absorb", |input| async move { Ok(input) });
    register_plugins(&mut registry, &[echo_entry()])
        .await
        .unwrap();

    let pipeline = for_each_pipeline("echo::reflect", "item");
    let result = compile_pipeline(&pipeline, &registry, CompileOptions::permissive());

    assert!(
        has_dynamic_boundary(&result),
        "an undeclared output stays Dynamic and cannot satisfy the array check"
    );
}

/// A plugin can only declare its output shape, so it cannot satisfy the
/// separate `has_complete_contract` gate, which also wants an input schema and
/// a confidence capability. Strict mode therefore still refuses plugin steps.
/// #129 resolved the output-typing half only; this pins the remaining gap.
#[tokio::test]
async fn strict_mode_still_rejects_a_plugin_step_for_its_incomplete_contract() {
    let mut registry = HandlerRegistry::new();
    registry.handler_value("sink::absorb", |input| async move { Ok(input) });
    let entries = vec![typed_echo_entry("typed", "echo::typed", string_array())];
    register_plugins(&mut registry, &entries).await.unwrap();

    let pipeline = for_each_pipeline("echo::typed", "line");
    let result = compile_pipeline(&pipeline, &registry, CompileOptions::strict());

    assert!(
        has_missing_contract(&result),
        "a declared output schema alone is not a complete contract; \
         plugins cannot yet declare input schema or confidence"
    );
}

/// A plugin that declares a shape it then fails to honour must fail its step
/// rather than corrupt the value flowing into downstream steps. The runner
/// enforces the contract; the raw handler closure does not.
#[tokio::test]
async fn a_plugin_violating_its_own_schema_fails_the_step() {
    let mut registry = HandlerRegistry::new();
    let entries = vec![typed_echo_entry(
        "typed",
        "echo::typed",
        ValueSchema::String,
    )];
    register_plugins(&mut registry, &entries).await.unwrap();

    let pipeline = load(
        r#"
pipeline: contract-check
steps:
  - step: fetch
    handler: echo::typed
"#,
    )
    .unwrap();

    let runner = Runner::new(Arc::new(registry));
    let crux = runner
        .run(&pipeline, serde_json::json!(["not", "a", "string"]))
        .await;

    let error = crux
        .value()
        .expect_err("array output must not satisfy a declared string schema");
    assert!(
        error.to_string().contains("output contract violated"),
        "expected a contract violation, got {error}"
    );
}
