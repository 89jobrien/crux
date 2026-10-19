//! Safety wire types — violation variants for harness change validation.

use thiserror::Error;

/// Why a safety policy rejected a proposed diff.
#[derive(Debug, Clone, Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_cap_exceeded_message() {
        let v = SafetyViolation::HardCapExceeded {
            resource: "memory_mb".into(),
            limit: 512,
            proposed: 1024,
        };
        assert_eq!(
            v.to_string(),
            "hard cap exceeded: memory_mb limit=512, proposed=1024"
        );
    }

    #[test]
    fn forbidden_syscall_message() {
        let v = SafetyViolation::ForbiddenSyscall {
            syscall: "ptrace".into(),
        };
        assert_eq!(v.to_string(), "forbidden syscall: ptrace");
    }

    #[test]
    fn custom_message() {
        let v = SafetyViolation::Custom {
            reason: "disallowed image".into(),
        };
        assert_eq!(v.to_string(), "policy violation: disallowed image");
    }

    #[test]
    fn hard_cap_exceeded_serde_round_trip() {
        let v = SafetyViolation::HardCapExceeded {
            resource: "cpu_millicores".into(),
            limit: 1000,
            proposed: 2000,
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: SafetyViolation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.to_string(), v.to_string());
    }

    #[test]
    fn forbidden_syscall_serde_round_trip() {
        let v = SafetyViolation::ForbiddenSyscall {
            syscall: "execve".into(),
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: SafetyViolation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.to_string(), v.to_string());
    }

    #[test]
    fn custom_serde_round_trip() {
        let v = SafetyViolation::Custom {
            reason: "forbidden registry".into(),
        };
        let json = serde_json::to_string(&v).unwrap();
        let back: SafetyViolation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.to_string(), v.to_string());
    }
}
