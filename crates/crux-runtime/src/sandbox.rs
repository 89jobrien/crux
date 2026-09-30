//! Declarative sandbox profiles for side-effecting handler invocations.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Resource and capability limits applied to one handler invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxProfile {
    #[serde(default)]
    pub read_paths: Vec<PathBuf>,
    #[serde(default)]
    pub write_paths: Vec<PathBuf>,
    #[serde(default)]
    pub env_allowlist: BTreeSet<String>,
    #[serde(default)]
    pub network_enabled: bool,
    pub timeout_ms: u64,
    pub memory_mb: u64,
    pub cpu_millis: u64,
    #[serde(default)]
    pub deterministic: bool,
}

/// Capabilities and resources requested by a handler invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxRequest {
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
    pub env_vars: Vec<String>,
    pub network_required: bool,
    pub timeout_ms: u64,
    pub memory_mb: u64,
    pub cpu_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SandboxViolation {
    #[error("read path is outside the sandbox allowlist: {path}")]
    ReadPathDenied { path: PathBuf },
    #[error("write path is outside the sandbox allowlist: {path}")]
    WritePathDenied { path: PathBuf },
    #[error("environment variable is outside the sandbox allowlist: {name}")]
    EnvironmentDenied { name: String },
    #[error("network access is disabled by the sandbox profile")]
    NetworkDenied,
    #[error("sandbox resource limit exceeded: {resource} limit={limit}, requested={requested}")]
    ResourceExceeded {
        resource: &'static str,
        limit: u64,
        requested: u64,
    },
    #[error("deterministic sandbox forbids {operation}")]
    DeterminismViolation { operation: &'static str },
}

impl SandboxProfile {
    pub fn validate(&self, request: &SandboxRequest) -> Result<(), SandboxViolation> {
        for path in &request.read_paths {
            if !path_is_allowed(path, &self.read_paths) {
                return Err(SandboxViolation::ReadPathDenied { path: path.clone() });
            }
        }
        for path in &request.write_paths {
            if !path_is_allowed(path, &self.write_paths) {
                return Err(SandboxViolation::WritePathDenied { path: path.clone() });
            }
        }
        for name in &request.env_vars {
            if !self.env_allowlist.contains(name) {
                return Err(SandboxViolation::EnvironmentDenied { name: name.clone() });
            }
        }
        if request.network_required && !self.network_enabled {
            return Err(SandboxViolation::NetworkDenied);
        }
        validate_resource("timeout_ms", self.timeout_ms, request.timeout_ms)?;
        validate_resource("memory_mb", self.memory_mb, request.memory_mb)?;
        validate_resource("cpu_millis", self.cpu_millis, request.cpu_millis)?;
        if self.deterministic {
            if !request.write_paths.is_empty() {
                return Err(SandboxViolation::DeterminismViolation {
                    operation: "writes",
                });
            }
            if request.network_required {
                return Err(SandboxViolation::DeterminismViolation {
                    operation: "network access",
                });
            }
        }
        Ok(())
    }
}

fn validate_resource(
    resource: &'static str,
    limit: u64,
    requested: u64,
) -> Result<(), SandboxViolation> {
    if requested > limit {
        return Err(SandboxViolation::ResourceExceeded {
            resource,
            limit,
            requested,
        });
    }
    Ok(())
}

fn path_is_allowed(path: &Path, allowed_roots: &[PathBuf]) -> bool {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return false;
    }
    allowed_roots.iter().any(|root| path.starts_with(root))
}
