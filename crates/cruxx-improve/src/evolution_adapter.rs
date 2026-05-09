//! Adapter: wraps EvolutionPlanner for the improvement protocol.

pub use cruxx_planner::evolution::EvolutionPlanner;
pub use cruxx_planner::metrics::RunMetrics;

use crate::improvement::StrategyDiff;
use cruxx_core::types::harness::HarnessProfile;

/// Convert an EvolutionPlanner proposal into a StrategyDiff.
///
/// This bridges the existing resource-level evolution (OOM/timeout bumps)
/// into the broader strategy improvement vocabulary.
pub fn evolution_to_strategy_diff(
    planner: &EvolutionPlanner,
    profile: &HarnessProfile,
    metrics: &[RunMetrics],
) -> StrategyDiff {
    let harness_diff = planner.propose(profile, metrics);
    if harness_diff.has_changes() {
        StrategyDiff {
            harness_diff: Some(harness_diff),
            ..Default::default()
        }
    } else {
        StrategyDiff::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cruxx_core::types::harness::{HarnessProfile, ResourceHints};

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

    #[test]
    fn healthy_metrics_produce_empty_diff() {
        let planner = EvolutionPlanner::default();
        let metrics = vec![RunMetrics {
            duration_ms: 800,
            peak_memory_mb: 200,
            exit_code: 0,
            success: true,
        }];
        let diff = evolution_to_strategy_diff(&planner, &base_profile(), &metrics);
        assert!(!diff.has_changes());
    }

    #[test]
    fn oom_produces_harness_diff() {
        let planner = EvolutionPlanner::default();
        let metrics = vec![RunMetrics {
            duration_ms: 1200,
            peak_memory_mb: 500,
            exit_code: 137,
            success: false,
        }];
        let diff = evolution_to_strategy_diff(&planner, &base_profile(), &metrics);
        assert!(diff.has_changes());
        assert!(diff.harness_diff.is_some());
        assert!(diff.harness_diff.unwrap().memory_delta_mb.unwrap() > 0);
    }
}
