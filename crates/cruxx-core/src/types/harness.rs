use serde::{Deserialize, Serialize};

/// Resource limits for a container execution environment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceHints {
    pub memory_mb: u64,
    pub cpu_millicores: u64,
    pub timeout_seconds: u64,
}

/// A named, versioned execution profile for container workloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessProfile {
    pub id: String,
    pub resources: ResourceHints,
    pub network_access: bool,
    pub allowed_syscalls: Vec<String>,
}

/// A proposed change to a HarnessProfile.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HarnessDiff {
    pub memory_delta_mb: Option<i64>,
    pub cpu_delta_millicores: Option<i64>,
    pub timeout_delta_seconds: Option<i64>,
    pub network_access_change: Option<bool>,
    pub syscall_additions: Vec<String>,
    pub syscall_removals: Vec<String>,
}

impl HarnessDiff {
    /// Returns true if any field contains a change.
    pub fn has_changes(&self) -> bool {
        self.memory_delta_mb.is_some()
            || self.cpu_delta_millicores.is_some()
            || self.timeout_delta_seconds.is_some()
            || self.network_access_change.is_some()
            || !self.syscall_additions.is_empty()
            || !self.syscall_removals.is_empty()
    }

    /// Apply this diff to a profile, producing a new profile.
    pub fn apply(&self, base: &HarnessProfile) -> HarnessProfile {
        let mut result = base.clone();
        if let Some(delta) = self.memory_delta_mb {
            result.resources.memory_mb = (result.resources.memory_mb as i64 + delta).max(0) as u64;
        }
        if let Some(delta) = self.cpu_delta_millicores {
            result.resources.cpu_millicores =
                (result.resources.cpu_millicores as i64 + delta).max(0) as u64;
        }
        if let Some(delta) = self.timeout_delta_seconds {
            result.resources.timeout_seconds =
                (result.resources.timeout_seconds as i64 + delta).max(0) as u64;
        }
        if let Some(net) = self.network_access_change {
            result.network_access = net;
        }
        for syscall in &self.syscall_additions {
            if !result.allowed_syscalls.contains(syscall) {
                result.allowed_syscalls.push(syscall.clone());
            }
        }
        result
            .allowed_syscalls
            .retain(|s| !self.syscall_removals.contains(s));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_profile_serde_round_trip() {
        let profile = HarnessProfile {
            id: "default-v1".to_string(),
            resources: ResourceHints {
                memory_mb: 512,
                cpu_millicores: 1000,
                timeout_seconds: 300,
            },
            network_access: false,
            allowed_syscalls: vec!["read".into(), "write".into(), "mmap".into()],
        };
        let json = serde_json::to_string(&profile).unwrap();
        let back: HarnessProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "default-v1");
        assert_eq!(back.resources.memory_mb, 512);
        assert!(!back.network_access);
    }

    #[test]
    fn harness_diff_fields_changed() {
        let diff = HarnessDiff {
            memory_delta_mb: Some(256),
            cpu_delta_millicores: None,
            timeout_delta_seconds: Some(60),
            network_access_change: Some(true),
            syscall_additions: vec!["connect".into()],
            syscall_removals: vec![],
        };
        assert!(diff.has_changes());
    }

    #[test]
    fn empty_diff_has_no_changes() {
        let diff = HarnessDiff::default();
        assert!(!diff.has_changes());
    }
}
