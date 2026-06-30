//! Improvement protocol for the crux agent runtime.
//!
//! Re-exports core trace types from `crux-types` and defines the
//! improvement vocabulary: strategies, diffs, comparisons, and policies.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Re-export trace types so downstream only needs `crux-improve`.
pub use crux_types::crux_value::Crux;
pub use crux_types::id::CruxId;
pub use crux_types::step::{Step, StepKind, StepStatus};

// ---------------------------------------------------------------------------
// TraceMetrics
// ---------------------------------------------------------------------------

const SUCCESS_WEIGHT: f32 = 0.60;
const CONFIDENCE_WEIGHT: f32 = 0.40;

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
    pub score: f32,
}

impl TraceMetrics {
    pub fn extract<T>(trace: &Crux<T>) -> Self {
        let step_count = trace.steps.len();
        let ok_count = trace.steps.iter().filter(|s| s.is_ok()).count();
        let error_count = trace.steps.iter().filter(|s| s.is_err()).count();

        let success_rate = if step_count > 0 {
            ok_count as f32 / step_count as f32
        } else {
            0.0
        };

        let avg_confidence = if step_count > 0 {
            trace.steps.iter().map(|s| s.confidence).sum::<f32>() / step_count as f32
        } else {
            0.0
        };

        let total_duration_ms = trace.steps.iter().map(|s| s.duration_ms).sum();

        let delegation_count = trace
            .steps
            .iter()
            .filter(|s| s.kind == StepKind::Delegation)
            .count();

        let delegation_depth = if trace.children.is_empty() {
            0
        } else {
            1 + trace
                .children
                .iter()
                .map(|c| Self::extract(c).delegation_depth)
                .max()
                .unwrap_or(0)
        };

        let speculation_count = trace
            .steps
            .iter()
            .filter(|s| s.kind == StepKind::Speculation)
            .count();

        let speculation_hit_count = trace
            .steps
            .iter()
            .filter(|s| s.kind == StepKind::Speculation && s.is_ok())
            .count();

        let speculation_hit_rate = if speculation_count > 0 {
            speculation_hit_count as f32 / speculation_count as f32
        } else {
            0.0
        };

        let score = SUCCESS_WEIGHT * success_rate + CONFIDENCE_WEIGHT * avg_confidence;

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
}

// ---------------------------------------------------------------------------
// Strategy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Strategy {
    pub version: u64,
    pub tool_preferences: HashMap<String, i32>,
    pub confidence_thresholds: HashMap<String, f32>,
    pub prompt_patches: Vec<PromptPatch>,
}

impl Strategy {
    pub fn apply(&mut self, diff: &StrategyDiff) {
        for (k, v) in &diff.tool_preferences {
            self.tool_preferences.insert(k.clone(), *v);
        }
        for (k, v) in &diff.confidence_thresholds {
            self.confidence_thresholds.insert(k.clone(), *v);
        }
        self.prompt_patches.extend(diff.prompt_patches.clone());
        self.version += 1;
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StrategyDiff {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_preferences: Vec<(String, i32)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub confidence_thresholds: Vec<(String, f32)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_patches: Vec<PromptPatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptPatch {
    pub agent: String,
    pub section: String,
    pub content: String,
}

// ---------------------------------------------------------------------------
// Improvement
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Improvement {
    pub id: CruxId,
    pub kind: ImprovementKind,
    pub target: String,
    pub diff: StrategyDiff,
    pub confidence: f32,
    pub evidence: Vec<String>,
    pub proposed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImprovementKind {
    ConfidenceThreshold,
    PromptTemplate,
    ToolPreference,
}

// ---------------------------------------------------------------------------
// Comparison / Verdict
// ---------------------------------------------------------------------------

const VERDICT_THRESHOLD: f32 = 0.05;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comparison {
    pub old_metrics: TraceMetrics,
    pub new_metrics: TraceMetrics,
    pub delta: f32,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Improved,
    Regressed,
    Neutral,
}

pub fn replay_compare<T>(old: &Crux<T>, new: &Crux<T>) -> Comparison {
    let old_metrics = TraceMetrics::extract(old);
    let new_metrics = TraceMetrics::extract(new);
    let delta = new_metrics.score - old_metrics.score;

    let verdict = if delta > VERDICT_THRESHOLD {
        Verdict::Improved
    } else if delta < -VERDICT_THRESHOLD {
        Verdict::Regressed
    } else {
        Verdict::Neutral
    };

    Comparison {
        old_metrics,
        new_metrics,
        delta,
        verdict,
    }
}

// ---------------------------------------------------------------------------
// StrategyPolicy
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
#[error("strategy violation: {message}")]
pub struct StrategyViolation {
    pub message: String,
}

pub trait StrategyPolicy: Send + Sync {
    fn validate_strategy(&self, diff: &StrategyDiff) -> Result<(), StrategyViolation>;
    fn requires_strategy_approval(&self, diff: &StrategyDiff) -> bool;
}

#[derive(Debug, Clone, Default)]
pub struct DefaultStrategyPolicy {
    pub max_tool_pref: Option<i32>,
}

impl StrategyPolicy for DefaultStrategyPolicy {
    fn validate_strategy(&self, diff: &StrategyDiff) -> Result<(), StrategyViolation> {
        if let Some(max) = self.max_tool_pref {
            for (name, val) in &diff.tool_preferences {
                if *val > max {
                    return Err(StrategyViolation {
                        message: format!("tool preference '{name}' value {val} exceeds max {max}"),
                    });
                }
            }
        }
        Ok(())
    }

    fn requires_strategy_approval(&self, diff: &StrategyDiff) -> bool {
        !diff.prompt_patches.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_trace() -> Crux<serde_json::Value> {
        Crux {
            id: CruxId::new(),
            agent: "test".into(),
            value: Ok(serde_json::json!({})),
            steps: vec![],
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        }
    }

    fn step(name: &str, status: StepStatus, confidence: f32) -> Step {
        Step {
            name: name.into(),
            kind: StepKind::Plain,
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
            findings: vec![],
        }
    }

    #[test]
    fn trace_metrics_empty() {
        let t = empty_trace();
        let m = TraceMetrics::extract(&t);
        assert_eq!(m.step_count, 0);
        assert_eq!(m.score, 0.0);
    }

    #[test]
    fn trace_metrics_computes_score() {
        let mut t = empty_trace();
        t.steps = vec![
            step("a", StepStatus::Ok, 0.8),
            step("b", StepStatus::Ok, 0.6),
        ];
        let m = TraceMetrics::extract(&t);
        assert_eq!(m.step_count, 2);
        assert!((m.success_rate - 1.0).abs() < f32::EPSILON);
        assert!((m.avg_confidence - 0.7).abs() < f32::EPSILON);
        let expected = SUCCESS_WEIGHT * 1.0 + CONFIDENCE_WEIGHT * 0.7;
        assert!((m.score - expected).abs() < 0.001);
    }

    #[test]
    fn strategy_apply_increments_version() {
        let mut s = Strategy::default();
        assert_eq!(s.version, 0);
        s.apply(&StrategyDiff {
            tool_preferences: vec![("rg".into(), 5)],
            ..Default::default()
        });
        assert_eq!(s.version, 1);
        assert_eq!(s.tool_preferences["rg"], 5);
    }

    #[test]
    fn replay_compare_detects_improvement() {
        let mut old = empty_trace();
        old.steps = vec![step("a", StepStatus::Err, 0.2)];
        let mut new = empty_trace();
        new.steps = vec![step("a", StepStatus::Ok, 0.9)];
        let cmp = replay_compare(&old, &new);
        assert_eq!(cmp.verdict, Verdict::Improved);
        assert!(cmp.delta > 0.0);
    }

    #[test]
    fn replay_compare_detects_regression() {
        let mut old = empty_trace();
        old.steps = vec![step("a", StepStatus::Ok, 0.9)];
        let mut new = empty_trace();
        new.steps = vec![step("a", StepStatus::Err, 0.2)];
        let cmp = replay_compare(&old, &new);
        assert_eq!(cmp.verdict, Verdict::Regressed);
    }

    #[test]
    fn default_policy_approves_tool_prefs() {
        let policy = DefaultStrategyPolicy::default();
        let diff = StrategyDiff {
            tool_preferences: vec![("rg".into(), 5)],
            ..Default::default()
        };
        assert!(policy.validate_strategy(&diff).is_ok());
        assert!(!policy.requires_strategy_approval(&diff));
    }

    #[test]
    fn default_policy_requires_approval_for_prompts() {
        let policy = DefaultStrategyPolicy::default();
        let diff = StrategyDiff {
            prompt_patches: vec![PromptPatch {
                agent: "test".into(),
                section: "system".into(),
                content: "be helpful".into(),
            }],
            ..Default::default()
        };
        assert!(policy.requires_strategy_approval(&diff));
    }

    #[test]
    fn strategy_serde_roundtrip() {
        let mut s = Strategy::default();
        s.tool_preferences.insert("rg".into(), 5);
        s.confidence_thresholds.insert("spec".into(), 0.7);
        let json = serde_json::to_string(&s).unwrap();
        let back: Strategy = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool_preferences["rg"], 5);
    }
}
