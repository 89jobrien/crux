use crux_plugin::bridge::register_plugins;
use crux_plugin::manifest::PluginEntry;
use cruxai_script::HandlerRegistry;
use std::collections::HashMap;

fn echo_entry() -> PluginEntry {
    let bin = env!("CARGO_BIN_EXE_echo-plugin");
    PluginEntry {
        name: "echo".into(),
        path: bin.into(),
        env: HashMap::new(),
    }
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
    let output = handler(input.clone()).await.unwrap();
    assert_eq!(output, input);
}
