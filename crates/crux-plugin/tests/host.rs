use crux_plugin::host::PluginHost;
use crux_plugin::manifest::PluginEntry;
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
async fn host_declares_handlers() {
    let mut host = PluginHost::new();
    host.load_plugin(&echo_entry()).await.unwrap();
    let handlers = host.declared_handlers();
    assert_eq!(handlers.len(), 1);
    assert_eq!(handlers[0].name, "echo::reflect");
}

#[tokio::test]
async fn host_invokes_handler() {
    let mut host = PluginHost::new();
    host.load_plugin(&echo_entry()).await.unwrap();
    let input = serde_json::json!({"hello": "world"});
    let output = host.invoke("echo::reflect", input.clone()).await.unwrap();
    assert_eq!(output, input);
}

#[tokio::test]
async fn host_invoke_unknown_handler_errors() {
    let mut host = PluginHost::new();
    host.load_plugin(&echo_entry()).await.unwrap();
    let result = host
        .invoke("echo::nonexistent", serde_json::json!({}))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn host_shutdown() {
    let mut host = PluginHost::new();
    host.load_plugin(&echo_entry()).await.unwrap();
    host.shutdown_all().await;
    // After shutdown, invoke should error
    let result = host.invoke("echo::reflect", serde_json::json!({})).await;
    assert!(result.is_err());
}
