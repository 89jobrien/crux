use crux_runtime::types::harness::{HarnessDiff, HarnessProfile};

use crate::metrics::RunMetrics;

/// Deterministic planner: given metrics history, propose a profile diff.
#[derive(Debug, Clone)]
pub struct EvolutionPlanner {
    /// If peak memory exceeds this fraction of limit, propose a bump.
    pub memory_pressure_threshold: f64,
    /// If duration exceeds this fraction of timeout, propose a bump.
    pub timeout_pressure_threshold: f64,
    /// How much to bump memory by (as fraction of current).
    pub memory_bump_factor: f64,
    /// How much to bump timeout by (as fraction of current).
    pub timeout_bump_factor: f64,
}

impl Default for EvolutionPlanner {
    fn default() -> Self {
        Self {
            memory_pressure_threshold: 0.9,
            timeout_pressure_threshold: 0.9,
            memory_bump_factor: 0.5,
            timeout_bump_factor: 0.5,
        }
    }
}

/// Linux OOM killer exit code.
const OOM_EXIT_CODE: i32 = 137;
/// Minimum memory bump (MB) after OOM.
const MIN_OOM_MEMORY_BUMP_MB: i64 = 128;
/// Minimum memory bump (MB) under pressure.
const MIN_PRESSURE_MEMORY_BUMP_MB: i64 = 64;
/// Minimum timeout bump (seconds).
const MIN_TIMEOUT_BUMP_SECONDS: i64 = 30;
/// Milliseconds per second.
const MS_PER_SECOND: u64 = 1000;

impl EvolutionPlanner {
    /// Analyze metrics and produce a diff. Returns empty diff if no changes needed.
    pub fn propose(&self, profile: &HarnessProfile, metrics: &[RunMetrics]) -> HarnessDiff {
        if metrics.is_empty() {
            return HarnessDiff::default();
        }

        let mut diff = HarnessDiff::default();

        // Check for OOM kills (exit code 137)
        let oom_count = metrics
            .iter()
            .filter(|m| m.exit_code == OOM_EXIT_CODE)
            .count();
        if oom_count > 0 {
            let bump = (profile.resources.memory_mb as f64 * self.memory_bump_factor) as i64;
            diff.memory_delta_mb = Some(bump.max(MIN_OOM_MEMORY_BUMP_MB));
        }

        // Check memory pressure (non-OOM but close to limit)
        if diff.memory_delta_mb.is_none() {
            let max_peak = metrics.iter().map(|m| m.peak_memory_mb).max().unwrap_or(0);
            let pressure = max_peak as f64 / profile.resources.memory_mb as f64;
            if pressure > self.memory_pressure_threshold {
                let bump = (profile.resources.memory_mb as f64 * self.memory_bump_factor) as i64;
                diff.memory_delta_mb = Some(bump.max(MIN_PRESSURE_MEMORY_BUMP_MB));
            }
        }

        // Check timeout pressure
        let timeout_ms = profile.resources.timeout_seconds * MS_PER_SECOND;
        let max_duration = metrics.iter().map(|m| m.duration_ms).max().unwrap_or(0);
        let time_pressure = max_duration as f64 / timeout_ms as f64;
        if time_pressure > self.timeout_pressure_threshold {
            let bump = (profile.resources.timeout_seconds as f64 * self.timeout_bump_factor) as i64;
            diff.timeout_delta_seconds = Some(bump.max(MIN_TIMEOUT_BUMP_SECONDS));
        }

        diff
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::RunMetrics;
    use crux_runtime::types::harness::{HarnessProfile, ResourceHints};

    fn base_profile() -> HarnessProfile {
        HarnessProfile {
            id: "default-v1".into(),
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
    fn propose_memory_bump_on_oom() {
        let metrics = vec![
            RunMetrics {
                duration_ms: 1200,
                peak_memory_mb: 500,
                exit_code: 137,
                success: false,
            },
            RunMetrics {
                duration_ms: 1100,
                peak_memory_mb: 510,
                exit_code: 137,
                success: false,
            },
        ];
        let planner = EvolutionPlanner::default();
        let diff = planner.propose(&base_profile(), &metrics);
        assert!(diff.memory_delta_mb.is_some());
        assert!(diff.memory_delta_mb.unwrap() > 0);
    }

    #[test]
    fn propose_no_change_when_healthy() {
        let metrics = vec![RunMetrics {
            duration_ms: 800,
            peak_memory_mb: 200,
            exit_code: 0,
            success: true,
        }];
        let planner = EvolutionPlanner::default();
        let diff = planner.propose(&base_profile(), &metrics);
        assert!(!diff.has_changes());
    }

    #[test]
    fn propose_timeout_bump_on_slow_runs() {
        let metrics = vec![
            RunMetrics {
                duration_ms: 290_000, // near 300s timeout
                peak_memory_mb: 200,
                exit_code: 0,
                success: true,
            },
            RunMetrics {
                duration_ms: 295_000,
                peak_memory_mb: 200,
                exit_code: 0,
                success: true,
            },
        ];
        let planner = EvolutionPlanner::default();
        let diff = planner.propose(&base_profile(), &metrics);
        assert!(diff.timeout_delta_seconds.is_some());
        assert!(diff.timeout_delta_seconds.unwrap() > 0);
    }
}
