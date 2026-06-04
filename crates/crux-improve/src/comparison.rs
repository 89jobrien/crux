use crate::metrics::TraceMetrics;
use crux_types::crux_value::Crux;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Improved,
    Regressed,
    Neutral,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comparison {
    pub verdict: Verdict,
    pub delta: f32,
    pub old_metrics: TraceMetrics,
    pub new_metrics: TraceMetrics,
}

/// Minimum score delta to consider a change meaningful.
const SIGNIFICANCE_THRESHOLD: f32 = 0.05;

/// Compare two execution traces and produce a verdict.
///
/// This is crux-domain logic: it knows what steps mean, how to weight
/// success rate vs confidence, and what constitutes a meaningful delta.
pub fn replay_compare(old: &Crux<serde_json::Value>, new: &Crux<serde_json::Value>) -> Comparison {
    let old_metrics = TraceMetrics::extract(old);
    let new_metrics = TraceMetrics::extract(new);
    let delta = new_metrics.score - old_metrics.score;

    let verdict = if delta > SIGNIFICANCE_THRESHOLD {
        Verdict::Improved
    } else if delta < -SIGNIFICANCE_THRESHOLD {
        Verdict::Regressed
    } else {
        Verdict::Neutral
    };

    Comparison {
        verdict,
        delta,
        old_metrics,
        new_metrics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::test_helpers::{step, trace};
    use crux_types::step::{StepKind, StepStatus};

    #[test]
    fn detects_improvement() {
        let old = trace(
            vec![step("a", StepStatus::Err, 0.3, StepKind::Plain)],
            vec![],
        );
        let new = trace(
            vec![step("a", StepStatus::Ok, 0.8, StepKind::Plain)],
            vec![],
        );
        let cmp = replay_compare(&old, &new);
        assert_eq!(cmp.verdict, Verdict::Improved);
        assert!(cmp.delta > 0.0);
    }

    #[test]
    fn detects_regression() {
        let old = trace(
            vec![step("a", StepStatus::Ok, 0.9, StepKind::Plain)],
            vec![],
        );
        let new = trace(
            vec![step("a", StepStatus::Err, 0.2, StepKind::Plain)],
            vec![],
        );
        let cmp = replay_compare(&old, &new);
        assert_eq!(cmp.verdict, Verdict::Regressed);
    }

    #[test]
    fn detects_neutral() {
        let old = trace(
            vec![step("a", StepStatus::Ok, 0.7, StepKind::Plain)],
            vec![],
        );
        let new = trace(
            vec![step("a", StepStatus::Ok, 0.72, StepKind::Plain)],
            vec![],
        );
        let cmp = replay_compare(&old, &new);
        assert_eq!(cmp.verdict, Verdict::Neutral);
    }

    #[test]
    fn comparison_includes_metric_deltas() {
        let old = trace(
            vec![
                step("a", StepStatus::Ok, 0.5, StepKind::Plain),
                step("b", StepStatus::Err, 0.3, StepKind::Plain),
            ],
            vec![],
        );
        let new = trace(
            vec![
                step("a", StepStatus::Ok, 0.9, StepKind::Plain),
                step("b", StepStatus::Ok, 0.8, StepKind::Plain),
            ],
            vec![],
        );
        let cmp = replay_compare(&old, &new);
        assert!(cmp.new_metrics.success_rate > cmp.old_metrics.success_rate);
        assert!(cmp.new_metrics.avg_confidence > cmp.old_metrics.avg_confidence);
    }
}
