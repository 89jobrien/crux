use crux_plugin::bridge::register_plugins;
use crux_plugin::manifest::PluginEntry;
use crux_script::HandlerRegistry;
use std::collections::HashMap;

fn echo_entry() -> PluginEntry {
    let bin = env!("CARGO_BIN_EXE_echo-plugin");
    PluginEntry {
        name: "echo".into(),
        path: bin.into(),
        env: HashMap::new(),
    }
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
