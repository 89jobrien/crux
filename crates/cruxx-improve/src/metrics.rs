use cruxx_types::crux_value::Crux;
use cruxx_types::step::{StepKind, StepStatus};
use serde::{Deserialize, Serialize};

/// Metrics extracted from a `Crux<T>` trace. Crux-domain knowledge —
/// consumers should not recompute these from raw steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceMetrics {
    pub step_count: usize,
    pub success_rate: f32,
    pub error_count: usize,
    pub avg_confidence: f32,
    pub total_duration_ms: u64,
    pub delegation_count: usize,
    pub delegation_depth: usize,
    pub speculation_count: usize,
    pub speculation_hit_count: usize,
    pub speculation_hit_rate: f32,
    /// Composite score: 60% success rate + 40% avg confidence.
    /// Returns 0.5 for empty traces.
    pub score: f32,
}

impl TraceMetrics {
    pub fn extract(trace: &Crux<serde_json::Value>) -> Self {
        let steps = &trace.steps;
        let step_count = steps.len();

        if step_count == 0 {
            return Self {
                step_count: 0,
                success_rate: 1.0,
                error_count: 0,
                avg_confidence: 0.0,
                total_duration_ms: 0,
                delegation_count: 0,
                delegation_depth: 0,
                speculation_count: 0,
                speculation_hit_count: 0,
                speculation_hit_rate: 0.0,
                score: 0.5,
            };
        }

        let ok_count = steps.iter().filter(|s| s.status == StepStatus::Ok).count();
        let error_count = steps.iter().filter(|s| s.status == StepStatus::Err).count();
        let success_rate = ok_count as f32 / step_count as f32;

        let avg_confidence = steps.iter().map(|s| s.confidence).sum::<f32>() / step_count as f32;

        let total_duration_ms = steps.iter().map(|s| s.duration_ms).sum();

        let delegation_count = steps
            .iter()
            .filter(|s| s.kind == StepKind::Delegation)
            .count();

        let delegation_depth = Self::max_depth(trace);

        let speculation_count = steps
            .iter()
            .filter(|s| s.kind == StepKind::Speculation)
            .count();
        let speculation_hit_count = steps
            .iter()
            .filter(|s| s.kind == StepKind::Speculation && s.status == StepStatus::Ok)
            .count();
        let speculation_hit_rate = if speculation_count > 0 {
            speculation_hit_count as f32 / speculation_count as f32
        } else {
            0.0
        };

        let score = success_rate * 0.6 + avg_confidence * 0.4;

        Self {
            step_count,
            success_rate,
            error_count,
            avg_confidence,
            total_duration_ms,
            delegation_count,
            delegation_depth,
            speculation_count,
            speculation_hit_count,
            speculation_hit_rate,
            score,
        }
    }

    fn max_depth(trace: &Crux<serde_json::Value>) -> usize {
        if trace.children.is_empty() {
            return 0;
        }
        trace
            .children
            .iter()
            .map(|c| 1 + Self::max_depth(c))
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use chrono::Utc;
    use cruxx_types::crux_value::Crux;
    use cruxx_types::id::CruxId;
    use cruxx_types::step::{Step, StepKind, StepStatus};

    pub fn step(name: &str, status: StepStatus, confidence: f32, kind: StepKind) -> Step {
        Step {
            name: name.into(),
            kind,
            status,
            confidence,
            started_at: Utc::now(),
            duration_ms: 100,
            input_hash: 0,
            content_hash: None,
            output: None,
            error: None,
            attempt: 1,
            events: vec![],
            metadata: Default::default(),
        }
    }

    pub fn trace(
        steps: Vec<Step>,
        children: Vec<Crux<serde_json::Value>>,
    ) -> Crux<serde_json::Value> {
        Crux {
            id: CruxId::new(),
            agent: "test".into(),
            value: Ok(serde_json::json!({})),
            steps,
            children,
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::{step, trace};
    use super::*;
    use cruxx_types::step::{StepKind, StepStatus};

    #[test]
    fn empty_trace_metrics() {
        let m = TraceMetrics::extract(&trace(vec![], vec![]));
        assert_eq!(m.step_count, 0);
        assert!((m.score - 0.5).abs() < f32::EPSILON);
        assert_eq!(m.delegation_depth, 0);
    }

    #[test]
    fn success_rate_computed_correctly() {
        let t = trace(
            vec![
                step("a", StepStatus::Ok, 0.8, StepKind::Plain),
                step("b", StepStatus::Err, 0.3, StepKind::Plain),
                step("c", StepStatus::Ok, 0.9, StepKind::Plain),
            ],
            vec![],
        );
        let m = TraceMetrics::extract(&t);
        assert!((m.success_rate - 2.0 / 3.0).abs() < 0.01);
        assert_eq!(m.error_count, 1);
    }

    #[test]
    fn delegation_depth_counts_nesting() {
        let child = trace(
            vec![step("inner", StepStatus::Ok, 0.7, StepKind::Plain)],
            vec![],
        );
        let parent = trace(
            vec![step("outer", StepStatus::Ok, 0.8, StepKind::Delegation)],
            vec![child],
        );
        let m = TraceMetrics::extract(&parent);
        assert_eq!(m.delegation_depth, 1);
        assert_eq!(m.delegation_count, 1);
    }

    #[test]
    fn speculation_stats() {
        let t = trace(
            vec![
                step("spec-a", StepStatus::Ok, 0.9, StepKind::Speculation),
                step("spec-b", StepStatus::Rejected, 0.4, StepKind::Speculation),
                step("plain", StepStatus::Ok, 0.7, StepKind::Plain),
            ],
            vec![],
        );
        let m = TraceMetrics::extract(&t);
        assert_eq!(m.speculation_count, 2);
        assert_eq!(m.speculation_hit_count, 1);
        assert!((m.speculation_hit_rate - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn metrics_serializable() {
        let m = TraceMetrics::extract(&trace(vec![], vec![]));
        let json = serde_json::to_string(&m).unwrap();
        let back: TraceMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(back.step_count, 0);
    }
}
