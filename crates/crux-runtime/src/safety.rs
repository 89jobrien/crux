use crate::types::harness::{HarnessDiff, HarnessProfile};
use thiserror::Error;

/// Why a safety policy rejected a proposed diff.
#[derive(Debug, Clone, Error)]
pub enum SafetyViolation {
    #[error("hard cap exceeded: {resource} limit={limit}, proposed={proposed}")]
    HardCapExceeded {
        resource: String,
        limit: u64,
        proposed: u64,
    },
    #[error("forbidden syscall: {syscall}")]
    ForbiddenSyscall { syscall: String },
    #[error("policy violation: {reason}")]
    Custom { reason: String },
}

/// Port: validates proposed harness changes against safety constraints.
pub trait SafetyPolicy: Send + Sync {
    /// Check whether a diff is safe to apply against the given base profile.
    fn validate(&self, diff: &HarnessDiff, base: &HarnessProfile) -> Result<(), SafetyViolation>;

    /// Returns true if this diff requires human/gate approval before applying.
    fn requires_approval(&self, diff: &HarnessDiff) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::harness::{HarnessDiff, HarnessProfile, ResourceHints};

    struct StrictPolicy {
        max_memory_mb: u64,
    }

    impl SafetyPolicy for StrictPolicy {
        fn validate(
            &self,
            diff: &HarnessDiff,
            base: &HarnessProfile,
        ) -> Result<(), SafetyViolation> {
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

    fn test_profile() -> HarnessProfile {
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

    #[test]
    fn validate_passes_within_limits() {
        let policy = StrictPolicy {
            max_memory_mb: 2048,
        };
        let diff = HarnessDiff {
            memory_delta_mb: Some(256),
            ..Default::default()
        };
        assert!(policy.validate(&diff, &test_profile()).is_ok());
    }

    #[test]
    fn validate_fails_above_hard_cap() {
        let policy = StrictPolicy { max_memory_mb: 600 };
        let diff = HarnessDiff {
            memory_delta_mb: Some(256),
            ..Default::default()
        };
        let result = policy.validate(&diff, &test_profile());
        assert!(matches!(
            result,
            Err(SafetyViolation::HardCapExceeded { .. })
        ));
    }

    #[test]
    fn requires_approval_for_network_escalation() {
        let policy = StrictPolicy {
            max_memory_mb: 4096,
        };
        let diff = HarnessDiff {
            network_access_change: Some(true),
            ..Default::default()
        };
        assert!(policy.requires_approval(&diff));
    }

    #[test]
    fn no_approval_for_resource_only_change() {
        let policy = StrictPolicy {
            max_memory_mb: 4096,
        };
        let diff = HarnessDiff {
            memory_delta_mb: Some(128),
            ..Default::default()
        };
        assert!(!policy.requires_approval(&diff));
    }
}
