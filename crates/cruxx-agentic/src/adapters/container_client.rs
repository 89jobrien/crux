use serde::{Deserialize, Serialize};
use std::future::Future;

/// Status of a container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerState {
    Running,
    Stopped { exit_code: i32 },
    Failed { error: String },
}

/// Result of starting a container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerHandle {
    pub container_id: String,
    pub state: ContainerState,
}

/// Port: manages container lifecycle.
pub trait ContainerClient: Send + Sync {
    fn run(
        &self,
        image: &str,
        cmd: &[String],
        memory_mb: u64,
        cpu_millicores: u64,
        timeout_seconds: u64,
    ) -> impl Future<Output = Result<ContainerHandle, String>> + Send;

    fn wait(
        &self,
        container_id: &str,
        timeout_seconds: u64,
    ) -> impl Future<Output = Result<ContainerState, String>> + Send;
}

/// Mock implementation for testing.
pub struct MockContainerClient;

impl ContainerClient for MockContainerClient {
    async fn run(
        &self,
        _image: &str,
        _cmd: &[String],
        _memory_mb: u64,
        _cpu_millicores: u64,
        _timeout_seconds: u64,
    ) -> Result<ContainerHandle, String> {
        Ok(ContainerHandle {
            container_id: "mock-container-001".into(),
            state: ContainerState::Running,
        })
    }

    async fn wait(
        &self,
        _container_id: &str,
        _timeout_seconds: u64,
    ) -> Result<ContainerState, String> {
        Ok(ContainerState::Stopped { exit_code: 0 })
    }
}
