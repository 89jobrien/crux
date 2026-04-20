use cruxx_core::prelude::CruxErr;
use cruxx_script::HandlerRegistry;
use serde_json::{Value, json};

use crate::adapters::container_client::ContainerClient;

#[cfg(feature = "docker")]
use crate::adapters::container_client::DockerContainerClient;

#[cfg(not(feature = "docker"))]
use crate::adapters::container_client::MockContainerClient;

/// Register container step handlers.
pub fn register(registry: &mut HandlerRegistry) {
    registry.handler("container::run", |input: Value| async move {
        handle_run(input).await
    });
    registry.handler("container::wait", |input: Value| async move {
        handle_wait(input).await
    });
}

/// Build the default container client for the current feature set.
///
/// - With `docker` feature: connects to the local Docker daemon.
/// - Without: uses `MockContainerClient` (test / CI safe).
#[cfg(feature = "docker")]
fn default_client() -> impl ContainerClient {
    DockerContainerClient::new().expect("failed to connect to Docker daemon")
}

#[cfg(not(feature = "docker"))]
fn default_client() -> impl ContainerClient {
    MockContainerClient
}

async fn handle_run(input: Value) -> Result<Value, CruxErr> {
    let args = input.get("args").unwrap_or(&input);
    let image = args["image"].as_str().unwrap_or("alpine:latest");
    let cmd: Vec<String> = args["cmd"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let memory_mb = args["memory_mb"].as_u64().unwrap_or(512);
    let cpu_millicores = args["cpu_millicores"].as_u64().unwrap_or(1000);
    let timeout = args["timeout_seconds"].as_u64().unwrap_or(300);

    let client = default_client();
    let handle = client
        .run(image, &cmd, memory_mb, cpu_millicores, timeout)
        .await
        .map_err(|e| CruxErr::step_failed("container::run", e))?;
    serde_json::to_value(&handle).map_err(|e| CruxErr::step_failed("container::run", e.to_string()))
}

async fn handle_wait(input: Value) -> Result<Value, CruxErr> {
    let args = input.get("args").unwrap_or(&input);
    let container_id = args["container_id"]
        .as_str()
        .ok_or_else(|| CruxErr::step_failed("container::wait", "missing container_id"))?;
    let timeout = args["timeout_seconds"].as_u64().unwrap_or(300);

    let client = default_client();
    let state = client
        .wait(container_id, timeout)
        .await
        .map_err(|e| CruxErr::step_failed("container::wait", e))?;
    serde_json::to_value(json!({"state": state}))
        .map_err(|e| CruxErr::step_failed("container::wait", e.to_string()))
}
