//! Bridge plugin handlers into `HandlerRegistry`.
//!
//! Wraps `PluginHost` in shared state so that type-erased handler
//! closures can invoke plugins at runtime.

use std::sync::Arc;

use cruxx_core::prelude::CruxErr;
use cruxx_script::HandlerRegistry;
use tokio::sync::Mutex;

use crate::host::{PluginError, PluginHost};
use crate::manifest::PluginEntry;

/// Load all plugins from the given entries and register their
/// handlers into the registry.
pub async fn register_plugins(
    registry: &mut HandlerRegistry,
    entries: &[PluginEntry],
) -> Result<(), PluginError> {
    let mut host = PluginHost::new();
    for entry in entries {
        host.load_plugin(entry).await?;
    }

    let handler_names: Vec<String> = host
        .declared_handlers()
        .iter()
        .map(|h| h.name.clone())
        .collect();

    let host = Arc::new(Mutex::new(host));

    for name in handler_names {
        let host = host.clone();
        let handler_name = name.clone();
        registry.handler_value(name, move |input: serde_json::Value| {
            let host = host.clone();
            let name = handler_name.clone();
            async move {
                let mut host = host.lock().await;
                host.invoke(&name, input)
                    .await
                    .map_err(|e| CruxErr::step_failed(&name, e.to_string()))
            }
        });
    }

    Ok(())
}
