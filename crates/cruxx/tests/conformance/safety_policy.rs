/// Conformance tests: SafetyPolicy port — contract verification via test-local adapter.
///
/// Verifies that any implementor of SafetyPolicy satisfies the port contract
/// for validate() and requires_approval() across the documented boundary cases.
use cruxx::prelude::{HarnessDiff, HarnessProfile, ResourceHints, SafetyPolicy, SafetyViolation};

// ── Test-local adapter ───────────────────────────────────────────────────────

struct BoundedPolicy {
    max_memory_mb: u64,
    forbidden_syscalls: Vec<String>,
}

impl SafetyPolicy for BoundedPolicy {
    fn validate(&self, diff: &HarnessDiff, base: &HarnessProfile) -> Result<(), SafetyViolation> {
        for syscall in &diff.syscall_additions {
            if self.forbidden_syscalls.contains(syscall) {
                return Err(SafetyViolation::ForbiddenSyscall {
                    syscall: syscall.clone(),
                });
            }
        }
        let proposed = diff.apply(base);
        if proposed.resources.memory_mb > self.max_memory_mb {
            return Err(SafetyViolation::HardCapExceeded {
                resource: "memory_mb".into(),
                limit: self.max_memory_mb,
                proposed: proposed.resources.memory_mb,
            });
        }
        Ok(())
    }

    fn requires_approval(&self, diff: &HarnessDiff) -> bool {
        diff.network_access_change == Some(true) || !diff.syscall_additions.is_empty()
    }
}

fn base_profile() -> HarnessProfile {
    HarnessProfile {
        id: "test-v1".into(),
        resources: ResourceHints {
            memory_mb: 512,
            cpu_millicores: 1000,
            timeout_seconds: 300,
        },
        network_access: false,
        allowed_syscalls: vec!["read".into(), "write".into()],
    }
}

fn policy() -> BoundedPolicy {
    BoundedPolicy {
        max_memory_mb: 2048,
        forbidden_syscalls: vec!["ptrace".into(), "kexec_load".into()],
    }
}

// ── validate() ──────────────────────────────────────────────────────────────

#[test]
fn conformance_safety_policy_validate_ok_within_limits() {
    let diff = HarnessDiff {
        memory_delta_mb: Some(256),
        ..Default::default()
    };
    assert!(policy().validate(&diff, &base_profile()).is_ok());
}

#[test]
fn conformance_safety_policy_validate_err_hard_cap_exceeded() {
    // base is 512 MB, cap is 2048 MB, delta pushes to 2049 MB
    let diff = HarnessDiff {
        memory_delta_mb: Some(1537),
        ..Default::default()
    };
    let result = policy().validate(&diff, &base_profile());
    assert!(
        matches!(result, Err(SafetyViolation::HardCapExceeded { .. })),
        "expected HardCapExceeded, got {result:?}"
    );
}

#[test]
fn conformance_safety_policy_validate_err_forbidden_syscall() {
    let diff = HarnessDiff {
        syscall_additions: vec!["ptrace".into()],
        ..Default::default()
    };
    let result = policy().validate(&diff, &base_profile());
    assert!(
        matches!(result, Err(SafetyViolation::ForbiddenSyscall { .. })),
        "expected ForbiddenSyscall, got {result:?}"
    );
}

// ── requires_approval() ─────────────────────────────────────────────────────

#[test]
fn conformance_safety_policy_requires_approval_true_for_network_escalation() {
    let diff = HarnessDiff {
        network_access_change: Some(true),
        ..Default::default()
    };
    assert!(policy().requires_approval(&diff));
}

#[test]
fn conformance_safety_policy_requires_approval_true_for_syscall_additions() {
    let diff = HarnessDiff {
        syscall_additions: vec!["connect".into()],
        ..Default::default()
    };
    assert!(policy().requires_approval(&diff));
}

#[test]
fn conformance_safety_policy_requires_approval_false_for_resource_only_change() {
    let diff = HarnessDiff {
        memory_delta_mb: Some(128),
        cpu_delta_millicores: Some(500),
        ..Default::default()
    };
    assert!(!policy().requires_approval(&diff));
}
