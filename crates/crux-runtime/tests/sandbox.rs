use std::path::PathBuf;

use crux_runtime::sandbox::{SandboxProfile, SandboxRequest, SandboxViolation};

fn restricted_profile() -> SandboxProfile {
    SandboxProfile {
        read_paths: vec![PathBuf::from("/workspace/input")],
        write_paths: vec![PathBuf::from("/workspace/output")],
        env_allowlist: ["PATH".to_owned()].into_iter().collect(),
        network_enabled: false,
        timeout_ms: 1_000,
        memory_mb: 256,
        cpu_millis: 500,
        deterministic: true,
    }
}

#[test]
fn sandbox_profile_accepts_requests_within_all_limits() {
    let request = SandboxRequest {
        read_paths: vec![PathBuf::from("/workspace/input/data.json")],
        env_vars: vec!["PATH".into()],
        timeout_ms: 500,
        memory_mb: 128,
        cpu_millis: 250,
        ..Default::default()
    };

    assert!(restricted_profile().validate(&request).is_ok());
}

#[test]
fn sandbox_profile_rejects_path_traversal_and_network_escalation() {
    let traversal = SandboxRequest {
        read_paths: vec![PathBuf::from("/workspace/input/../secret")],
        ..Default::default()
    };
    assert!(matches!(
        restricted_profile().validate(&traversal),
        Err(SandboxViolation::ReadPathDenied { .. })
    ));

    let network = SandboxRequest {
        network_required: true,
        ..Default::default()
    };
    assert!(matches!(
        restricted_profile().validate(&network),
        Err(SandboxViolation::NetworkDenied)
    ));
}

#[test]
fn deterministic_profile_rejects_writes() {
    let request = SandboxRequest {
        write_paths: vec![PathBuf::from("/workspace/output/result.json")],
        ..Default::default()
    };
    assert!(matches!(
        restricted_profile().validate(&request),
        Err(SandboxViolation::DeterminismViolation { .. })
    ));
}
