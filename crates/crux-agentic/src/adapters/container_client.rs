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

/// Docker adapter backed by the bollard crate.
///
/// Connect via the default Docker socket (respects `DOCKER_HOST` env var).
/// Resource limits are mapped as follows:
/// - `memory_mb`      → `HostConfig.memory` (bytes)
/// - `cpu_millicores` → `HostConfig.nano_cpus`
///                      (1 CPU = 1_000_000_000 nano CPUs; 1 millicore = 1_000_000 nano CPUs)
#[cfg(feature = "docker")]
pub struct DockerContainerClient {
    docker: bollard::Docker,
}

#[cfg(feature = "docker")]
impl DockerContainerClient {
    /// Connect to the local Docker daemon using the default socket / env vars.
    pub fn new() -> Result<Self, String> {
        let docker = bollard::Docker::connect_with_local_defaults()
            .map_err(|e| format!("docker connect: {e}"))?;
        Ok(Self { docker })
    }
}

#[cfg(feature = "docker")]
impl Default for DockerContainerClient {
    fn default() -> Self {
        Self::new().expect("failed to connect to Docker daemon")
    }
}

#[cfg(feature = "docker")]
impl ContainerClient for DockerContainerClient {
    async fn run(
        &self,
        image: &str,
        cmd: &[String],
        memory_mb: u64,
        cpu_millicores: u64,
        _timeout_seconds: u64,
    ) -> Result<ContainerHandle, String> {
        use bollard::models::{ContainerCreateBody, HostConfig};
        use bollard::query_parameters::{CreateContainerOptionsBuilder, CreateImageOptionsBuilder};
        use futures::TryStreamExt;

        // Pull image if not present (best-effort; ignore errors).
        let _ = self
            .docker
            .create_image(
                Some(
                    CreateImageOptionsBuilder::default()
                        .from_image(image)
                        .build(),
                ),
                None,
                None,
            )
            .try_collect::<Vec<_>>()
            .await;

        const BYTES_PER_MB: u64 = 1024 * 1024;
        const NANOCPUS_PER_MILLICORE: u64 = 1_000_000;

        let host_config = HostConfig {
            memory: Some((memory_mb * BYTES_PER_MB) as i64),
            nano_cpus: Some((cpu_millicores * NANOCPUS_PER_MILLICORE) as i64),
            ..Default::default()
        };

        let body = ContainerCreateBody {
            image: Some(image.to_owned()),
            cmd: if cmd.is_empty() {
                None
            } else {
                Some(cmd.to_vec())
            },
            host_config: Some(host_config),
            ..Default::default()
        };

        let container = self
            .docker
            .create_container(Some(CreateContainerOptionsBuilder::default().build()), body)
            .await
            .map_err(|e| format!("create container: {e}"))?;

        let container_id = container.id.clone();

        self.docker
            .start_container(&container_id, None)
            .await
            .map_err(|e| format!("start container: {e}"))?;

        Ok(ContainerHandle {
            container_id,
            state: ContainerState::Running,
        })
    }

    async fn wait(
        &self,
        container_id: &str,
        timeout_seconds: u64,
    ) -> Result<ContainerState, String> {
        use bollard::query_parameters::WaitContainerOptionsBuilder;
        use futures::StreamExt;
        use tokio::time::{Duration, timeout};

        let wait_fut = async {
            let mut stream = self.docker.wait_container(
                container_id,
                Some(
                    WaitContainerOptionsBuilder::default()
                        .condition("not-running")
                        .build(),
                ),
            );

            match stream.next().await {
                Some(Ok(response)) => {
                    let code = response.status_code as i32;
                    Ok(ContainerState::Stopped { exit_code: code })
                }
                Some(Err(e)) => Err(format!("wait error: {e}")),
                None => Ok(ContainerState::Stopped { exit_code: 0 }),
            }
        };

        timeout(Duration::from_secs(timeout_seconds), wait_fut)
            .await
            .unwrap_or_else(|_| {
                Err(format!(
                    "container {container_id} did not finish within {timeout_seconds}s"
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_run_returns_running_handle() {
        let client = MockContainerClient;
        let handle = client
            .run("alpine:latest", &[], 512, 1000, 30)
            .await
            .unwrap();
        assert_eq!(handle.state, ContainerState::Running);
        assert!(!handle.container_id.is_empty());
    }

    #[tokio::test]
    async fn mock_wait_returns_stopped_zero() {
        let client = MockContainerClient;
        let state = client.wait("mock-container-001", 30).await.unwrap();
        assert_eq!(state, ContainerState::Stopped { exit_code: 0 });
    }

    /// Smoke-test: DockerContainerClient runs alpine echo and waits for exit.
    /// Marked `ignore` so it only runs when a live Docker daemon is present.
    #[cfg(feature = "docker")]
    #[tokio::test]
    #[ignore = "requires a running Docker daemon"]
    async fn docker_run_and_wait_alpine_echo() {
        let client = DockerContainerClient::new().expect("docker connect");
        let cmd = vec!["echo".into(), "hello".into()];
        let handle = client
            .run("alpine:latest", &cmd, 64, 500, 30)
            .await
            .unwrap();
        assert_eq!(handle.state, ContainerState::Running);

        let final_state = client.wait(&handle.container_id, 30).await.unwrap();
        assert_eq!(final_state, ContainerState::Stopped { exit_code: 0 });
    }
}
