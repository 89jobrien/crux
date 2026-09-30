//! Improvement protocol for the crux agent runtime.
//!
//! Re-exports core trace types from `crux-types` and defines the
//! improvement vocabulary: strategies, diffs, comparisons, and policies.

// TODO(feature-idea-16): Integrate persisted runs with proposals, approval, and replay comparison.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Re-export trace types so downstream only needs `crux-improve`.
pub use crux_schema::crux_value::Crux;
pub use crux_types::id::CruxId;
pub use crux_types::step::{Step, StepKind, StepOrigin, StepStatus};

const SUCCESS_WEIGHT: f32 = 0.60;
const CONFIDENCE_WEIGHT: f32 = 0.40;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Computes success, confidence, timing, delegation, and speculation metrics.
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
            trace
                .steps
                .iter()
                .map(|step| {
                    if step.confidence.is_finite() && (0.0..=1.0).contains(&step.confidence) {
                        step.confidence
                    } else {
                        0.0
                    }
                })
                .sum::<f32>()
                / step_count as f32
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Strategy {
    pub version: u64,
    pub tool_preferences: HashMap<String, i32>,
    pub confidence_thresholds: HashMap<String, f32>,
    pub prompt_patches: Vec<PromptPatch>,
}

impl Strategy {
    /// Applies a strategy diff and increments the strategy version.
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

const VERDICT_THRESHOLD: f32 = 0.05;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Compares two traces and classifies the score delta against the verdict threshold.
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

/// Regression thresholds applied by [`EvalHarness`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EvalThresholds {
    /// Lowest acceptable candidate score delta relative to the baseline.
    pub min_score_delta: f32,
    /// Highest acceptable candidate-to-baseline duration ratio.
    pub max_latency_ratio: f32,
}

/// Serializable representation of candidate-to-baseline latency.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "ratio", rename_all = "snake_case")]
pub enum LatencyRatio {
    /// A finite candidate-to-baseline duration ratio.
    Finite(f32),
    /// The baseline duration was zero while the candidate duration was nonzero.
    Unbounded,
}

impl Default for LatencyRatio {
    fn default() -> Self {
        Self::Finite(1.0)
    }
}

/// Latency values captured during trace evaluation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LatencyComparison {
    pub baseline_ms: u64,
    pub candidate_ms: u64,
    pub ratio: LatencyRatio,
}

/// Invalid evaluation configuration.
#[derive(Debug, thiserror::Error)]
pub enum EvalConfigError {
    #[error("{field} must be finite, got {value}")]
    NonFiniteThreshold { field: &'static str, value: f32 },
    #[error("confidence tolerance must be non-negative, got {0}")]
    NegativeConfidenceTolerance(f32),
    #[error("maximum latency ratio must be non-negative, got {0}")]
    NegativeLatencyRatio(f32),
    #[error("candidate trace contains replayed step '{step}' at {path:?}")]
    ReplayedCandidateStep { path: TracePath, step: String },
    #[error("duplicate invariant id '{0}'")]
    DuplicateInvariantId(String),
    #[error("invariant '{id}' has invalid configuration: {message}")]
    InvalidInvariant { id: String, message: String },
    #[error("{role} step '{step}' at {path:?} has invalid confidence {value}")]
    InvalidStepConfidence {
        role: &'static str,
        path: TracePath,
        step: String,
        value: f32,
    },
    #[error("trace serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Reusable trace evaluation harness for candidate pipeline or model runs.
#[derive(Debug, Clone)]
pub struct EvalHarness {
    thresholds: EvalThresholds,
    golden_answer: Option<serde_json::Value>,
}

/// Result of evaluating a candidate trace against a baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalReport {
    pub passed: bool,
    pub comparison: Comparison,
    /// Backward-compatible finite latency ratio. Unbounded latency uses `f32::MAX`.
    #[serde(default = "default_latency_ratio")]
    pub latency_ratio: f32,
    #[serde(default)]
    pub latency: LatencyComparison,
    pub failures: Vec<String>,
}

impl EvalHarness {
    /// Constructs a harness while preserving the pre-0.4 infallible API.
    pub fn new(thresholds: EvalThresholds) -> Self {
        Self {
            thresholds,
            golden_answer: None,
        }
    }

    /// Constructs a harness after validating all thresholds.
    pub fn try_new(thresholds: EvalThresholds) -> Result<Self, EvalConfigError> {
        validate_eval_thresholds(thresholds)?;
        Ok(Self::new(thresholds))
    }

    pub fn with_golden_answer(mut self, answer: serde_json::Value) -> Self {
        self.golden_answer = Some(answer);
        self
    }

    pub fn evaluate<T: Serialize>(&self, baseline: &Crux<T>, candidate: &Crux<T>) -> EvalReport {
        let comparison = replay_compare(baseline, candidate);
        let baseline_latency = comparison.old_metrics.total_duration_ms;
        let candidate_latency = comparison.new_metrics.total_duration_ms;
        let latency_ratio = if baseline_latency == 0 {
            if candidate_latency == 0 {
                LatencyRatio::Finite(1.0)
            } else {
                LatencyRatio::Unbounded
            }
        } else {
            LatencyRatio::Finite(candidate_latency as f32 / baseline_latency as f32)
        };
        let mut failures = validate_eval_thresholds(self.thresholds)
            .err()
            .map(|error| vec![error.to_string()])
            .unwrap_or_default();
        if comparison.delta < self.thresholds.min_score_delta {
            failures.push(format!(
                "quality delta {} is below {}",
                comparison.delta, self.thresholds.min_score_delta
            ));
        }
        match latency_ratio {
            LatencyRatio::Finite(ratio) if ratio > self.thresholds.max_latency_ratio => {
                failures.push(format!(
                    "latency ratio {ratio} exceeds {}",
                    self.thresholds.max_latency_ratio
                ));
            }
            LatencyRatio::Unbounded => failures.push(format!(
                "latency ratio is unbounded and exceeds {}",
                self.thresholds.max_latency_ratio
            )),
            LatencyRatio::Finite(_) => {}
        }
        if let Some(expected) = &self.golden_answer {
            match candidate.value.as_ref() {
                Ok(value) => match serde_json::to_value(value) {
                    Ok(actual) if &actual == expected => {}
                    Ok(_) => failures.push("candidate did not match golden answer".into()),
                    Err(error) => failures.push(format!(
                        "candidate output could not be serialized for golden comparison: {error}"
                    )),
                },
                Err(_) => failures.push("candidate did not produce a golden answer".into()),
            }
        }
        let legacy_latency_ratio = match latency_ratio {
            LatencyRatio::Finite(ratio) => ratio,
            LatencyRatio::Unbounded => f32::MAX,
        };
        EvalReport {
            passed: failures.is_empty(),
            comparison,
            latency_ratio: legacy_latency_ratio,
            latency: LatencyComparison {
                baseline_ms: baseline_latency,
                candidate_ms: candidate_latency,
                ratio: latency_ratio,
            },
            failures,
        }
    }
}

fn validate_eval_thresholds(thresholds: EvalThresholds) -> Result<(), EvalConfigError> {
    if !thresholds.min_score_delta.is_finite() {
        return Err(EvalConfigError::NonFiniteThreshold {
            field: "min_score_delta",
            value: thresholds.min_score_delta,
        });
    }
    if !thresholds.max_latency_ratio.is_finite() {
        return Err(EvalConfigError::NonFiniteThreshold {
            field: "max_latency_ratio",
            value: thresholds.max_latency_ratio,
        });
    }
    if thresholds.max_latency_ratio < 0.0 {
        return Err(EvalConfigError::NegativeLatencyRatio(
            thresholds.max_latency_ratio,
        ));
    }
    Ok(())
}

fn default_latency_ratio() -> f32 {
    1.0
}

/// Structural comparison mode for execution traces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDiffMode {
    Strict,
    Compatible,
}

/// Controls whether candidate traces may contain replayed steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateTracePolicy {
    LiveOnly,
    AllowReplayed,
}

/// Policy for structural trace comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuralPolicy {
    pub mode: TraceDiffMode,
    pub confidence_tolerance: f32,
    pub compare_outputs: bool,
    pub candidate_trace: CandidateTracePolicy,
}

/// Index path from a root trace through its serialized child traces.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TracePath(pub Vec<usize>);

/// Stable or name-based identity for a recorded step.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum StepIdentity {
    StableId(String),
    Name(String),
}

/// A field-level structural change between two steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "field", content = "values", rename_all = "snake_case")]
pub enum StepChange {
    Added,
    Removed,
    Kind {
        baseline: StepKind,
        candidate: StepKind,
    },
    Status {
        baseline: StepStatus,
        candidate: StepStatus,
    },
    Confidence {
        baseline: f32,
        candidate: f32,
    },
    InputHash {
        baseline: u64,
        candidate: u64,
    },
    ContentHash {
        baseline: Option<u64>,
        candidate: Option<u64>,
    },
    Output {
        baseline: Option<serde_json::Value>,
        candidate: Option<serde_json::Value>,
    },
    Error {
        baseline: Option<String>,
        candidate: Option<String>,
    },
    Attempt {
        baseline: u32,
        candidate: u32,
    },
}

/// Structural changes for one logical step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepDiff {
    pub trace_path: TracePath,
    pub baseline_index: Option<usize>,
    pub candidate_index: Option<usize>,
    pub identity: StepIdentity,
    pub changes: Vec<StepChange>,
}

/// Deterministic structural comparison result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceDiff {
    pub mode: TraceDiffMode,
    pub matches: bool,
    pub step_diffs: Vec<StepDiff>,
}

/// Cardinality semantics for a step invariant selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOccurrence {
    ExactlyOne,
    AtLeastOne,
    All,
    Nth(usize),
}

/// Selects steps in one serialized trace node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepSelector {
    pub trace_path: TracePath,
    pub identity: StepIdentity,
    pub occurrence: StepOccurrence,
}

/// Required status and confidence for selected steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepInvariant {
    pub id: String,
    pub selector: StepSelector,
    pub expected_status: StepStatus,
    pub minimum_confidence: Option<f32>,
}

/// Combined metric, structural, output, and invariant policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegressionPolicy {
    pub metrics: EvalThresholds,
    pub structure: StructuralPolicy,
    pub expected_output: Option<serde_json::Value>,
    pub invariants: Vec<StepInvariant>,
}

/// One invariant that a candidate trace failed to satisfy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvariantViolation {
    pub invariant_id: String,
    pub selector: StepSelector,
    pub message: String,
}

/// Complete deterministic evaluation of a candidate trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegressionEvaluation {
    pub passed: bool,
    pub metrics: EvalReport,
    pub structure: TraceDiff,
    pub invariant_violations: Vec<InvariantViolation>,
    pub failures: Vec<String>,
}

/// Compares two traces under a validated structural policy.
pub fn diff_traces<T: Serialize>(
    baseline: &Crux<T>,
    candidate: &Crux<T>,
    policy: &StructuralPolicy,
) -> Result<TraceDiff, EvalConfigError> {
    validate_structural_policy(policy)?;
    validate_trace_confidences(baseline, "baseline", &TracePath(Vec::new()))?;
    validate_trace_confidences(candidate, "candidate", &TracePath(Vec::new()))?;
    if policy.candidate_trace == CandidateTracePolicy::LiveOnly {
        validate_live_candidate(candidate, &TracePath(Vec::new()))?;
    }

    let mut step_diffs = Vec::new();
    let matches = match policy.mode {
        TraceDiffMode::Strict => {
            let shape_matches = diff_strict_node(
                baseline,
                candidate,
                policy,
                &TracePath(Vec::new()),
                &mut step_diffs,
            )?;
            shape_matches && step_diffs.is_empty()
        }
        TraceDiffMode::Compatible => diff_compatible_node(
            baseline,
            candidate,
            policy,
            &TracePath(Vec::new()),
            &mut step_diffs,
        )?,
    };

    Ok(TraceDiff {
        mode: policy.mode,
        matches,
        step_diffs,
    })
}

/// Evaluates metric, structural, expected-output, and invariant policies.
pub fn evaluate_regression<T: Serialize>(
    baseline: &Crux<T>,
    candidate: &Crux<T>,
    policy: &RegressionPolicy,
) -> Result<RegressionEvaluation, EvalConfigError> {
    validate_regression_policy(policy)?;
    let mut harness = EvalHarness::try_new(policy.metrics)?;
    if let Some(expected) = &policy.expected_output {
        harness = harness.with_golden_answer(expected.clone());
    }
    let metrics = harness.evaluate(baseline, candidate);
    let structure = diff_traces(baseline, candidate, &policy.structure)?;
    let invariant_violations = evaluate_invariants(candidate, &policy.invariants);
    let mut failures = metrics.failures.clone();
    if !structure.matches {
        failures.push("candidate trace violated structural policy".into());
    }
    failures.extend(
        invariant_violations
            .iter()
            .map(|violation| violation.message.clone()),
    );

    Ok(RegressionEvaluation {
        passed: failures.is_empty(),
        metrics,
        structure,
        invariant_violations,
        failures,
    })
}

fn validate_structural_policy(policy: &StructuralPolicy) -> Result<(), EvalConfigError> {
    if !policy.confidence_tolerance.is_finite() {
        return Err(EvalConfigError::NonFiniteThreshold {
            field: "confidence_tolerance",
            value: policy.confidence_tolerance,
        });
    }
    if policy.confidence_tolerance < 0.0 {
        return Err(EvalConfigError::NegativeConfidenceTolerance(
            policy.confidence_tolerance,
        ));
    }
    Ok(())
}

fn validate_live_candidate<T>(trace: &Crux<T>, path: &TracePath) -> Result<(), EvalConfigError> {
    for step in &trace.steps {
        if step.origin == StepOrigin::Replayed {
            return Err(EvalConfigError::ReplayedCandidateStep {
                path: path.clone(),
                step: step.name.clone(),
            });
        }
    }
    for (index, child) in trace.children.iter().enumerate() {
        let mut child_path = path.0.clone();
        child_path.push(index);
        validate_live_candidate(child, &TracePath(child_path))?;
    }
    Ok(())
}

fn validate_trace_confidences<T>(
    trace: &Crux<T>,
    role: &'static str,
    path: &TracePath,
) -> Result<(), EvalConfigError> {
    for step in &trace.steps {
        if !step.confidence.is_finite() || !(0.0..=1.0).contains(&step.confidence) {
            return Err(EvalConfigError::InvalidStepConfidence {
                role,
                path: path.clone(),
                step: step.name.clone(),
                value: step.confidence,
            });
        }
    }
    for (index, child) in trace.children.iter().enumerate() {
        let mut child_path = path.0.clone();
        child_path.push(index);
        validate_trace_confidences(child, role, &TracePath(child_path))?;
    }
    Ok(())
}

fn step_identity<T>(step: &Step<T>) -> StepIdentity {
    step.stable_id.as_ref().map_or_else(
        || StepIdentity::Name(step.name.clone()),
        |stable_id| StepIdentity::StableId(stable_id.clone()),
    )
}

fn diff_strict_node<T: Serialize>(
    baseline: &Crux<T>,
    candidate: &Crux<T>,
    policy: &StructuralPolicy,
    path: &TracePath,
    diffs: &mut Vec<StepDiff>,
) -> Result<bool, EvalConfigError> {
    let common_len = baseline.steps.len().min(candidate.steps.len());
    for index in 0..common_len {
        let baseline_step = &baseline.steps[index];
        let candidate_step = &candidate.steps[index];
        let baseline_identity = step_identity(baseline_step);
        let candidate_identity = step_identity(candidate_step);
        if baseline_identity != candidate_identity {
            diffs.push(StepDiff {
                trace_path: path.clone(),
                baseline_index: Some(index),
                candidate_index: None,
                identity: baseline_identity,
                changes: vec![StepChange::Removed],
            });
            diffs.push(StepDiff {
                trace_path: path.clone(),
                baseline_index: None,
                candidate_index: Some(index),
                identity: candidate_identity,
                changes: vec![StepChange::Added],
            });
            continue;
        }

        let changes = compare_step_fields(baseline_step, candidate_step, policy)?;
        if !changes.is_empty() {
            diffs.push(StepDiff {
                trace_path: path.clone(),
                baseline_index: Some(index),
                candidate_index: Some(index),
                identity: baseline_identity,
                changes,
            });
        }
    }

    for (index, step) in baseline.steps.iter().enumerate().skip(common_len) {
        diffs.push(StepDiff {
            trace_path: path.clone(),
            baseline_index: Some(index),
            candidate_index: None,
            identity: step_identity(step),
            changes: vec![StepChange::Removed],
        });
    }
    for (index, step) in candidate.steps.iter().enumerate().skip(common_len) {
        diffs.push(StepDiff {
            trace_path: path.clone(),
            baseline_index: None,
            candidate_index: Some(index),
            identity: step_identity(step),
            changes: vec![StepChange::Added],
        });
    }

    let common_children = baseline.children.len().min(candidate.children.len());
    let mut shape_matches = baseline.children.len() == candidate.children.len();
    for index in 0..common_children {
        let mut child_path = path.0.clone();
        child_path.push(index);
        shape_matches &= diff_strict_node(
            &baseline.children[index],
            &candidate.children[index],
            policy,
            &TracePath(child_path),
            diffs,
        )?;
    }
    for (index, child) in baseline.children.iter().enumerate().skip(common_children) {
        let mut child_path = path.0.clone();
        child_path.push(index);
        record_subtree(child, &TracePath(child_path), StepChange::Removed, diffs);
    }
    for (index, child) in candidate.children.iter().enumerate().skip(common_children) {
        let mut child_path = path.0.clone();
        child_path.push(index);
        record_subtree(child, &TracePath(child_path), StepChange::Added, diffs);
    }
    Ok(shape_matches)
}

fn diff_compatible_node<T: Serialize>(
    baseline: &Crux<T>,
    candidate: &Crux<T>,
    policy: &StructuralPolicy,
    path: &TracePath,
    diffs: &mut Vec<StepDiff>,
) -> Result<bool, EvalConfigError> {
    let mut candidate_cursor = 0;
    let mut matches = true;

    for (baseline_index, baseline_step) in baseline.steps.iter().enumerate() {
        let identity = step_identity(baseline_step);
        let matched = candidate.steps[candidate_cursor..]
            .iter()
            .position(|step| step_identity(step) == identity)
            .map(|relative| candidate_cursor + relative);

        let Some(candidate_index) = matched else {
            diffs.push(StepDiff {
                trace_path: path.clone(),
                baseline_index: Some(baseline_index),
                candidate_index: None,
                identity,
                changes: vec![StepChange::Removed],
            });
            matches = false;
            continue;
        };

        for (index, added) in candidate.steps[candidate_cursor..candidate_index]
            .iter()
            .enumerate()
        {
            let absolute_index = candidate_cursor + index;
            diffs.push(StepDiff {
                trace_path: path.clone(),
                baseline_index: None,
                candidate_index: Some(absolute_index),
                identity: step_identity(added),
                changes: vec![StepChange::Added],
            });
            matches &= added.status == StepStatus::Ok;
        }

        let candidate_step = &candidate.steps[candidate_index];
        let changes = compare_compatible_fields(baseline_step, candidate_step, policy)?;
        if !changes.is_empty() {
            matches = false;
            diffs.push(StepDiff {
                trace_path: path.clone(),
                baseline_index: Some(baseline_index),
                candidate_index: Some(candidate_index),
                identity,
                changes,
            });
        }
        candidate_cursor = candidate_index + 1;
    }

    for (index, added) in candidate.steps.iter().enumerate().skip(candidate_cursor) {
        diffs.push(StepDiff {
            trace_path: path.clone(),
            baseline_index: None,
            candidate_index: Some(index),
            identity: step_identity(added),
            changes: vec![StepChange::Added],
        });
        matches &= added.status == StepStatus::Ok;
    }

    for (index, baseline_child) in baseline.children.iter().enumerate() {
        let mut child_path = path.0.clone();
        child_path.push(index);
        let child_path = TracePath(child_path);
        let Some(candidate_child) = candidate.children.get(index) else {
            record_subtree(baseline_child, &child_path, StepChange::Removed, diffs);
            matches = false;
            continue;
        };
        matches &=
            diff_compatible_node(baseline_child, candidate_child, policy, &child_path, diffs)?;
    }
    for (index, candidate_child) in candidate
        .children
        .iter()
        .enumerate()
        .skip(baseline.children.len())
    {
        let mut child_path = path.0.clone();
        child_path.push(index);
        record_subtree(
            candidate_child,
            &TracePath(child_path),
            StepChange::Added,
            diffs,
        );
        matches &= subtree_all_ok(candidate_child);
    }

    Ok(matches)
}

fn compare_compatible_fields<T: Serialize>(
    baseline: &Step<T>,
    candidate: &Step<T>,
    policy: &StructuralPolicy,
) -> Result<Vec<StepChange>, EvalConfigError> {
    let mut changes = Vec::new();
    if baseline.kind != candidate.kind {
        changes.push(StepChange::Kind {
            baseline: baseline.kind,
            candidate: candidate.kind,
        });
    }
    if baseline.status != candidate.status {
        changes.push(StepChange::Status {
            baseline: baseline.status,
            candidate: candidate.status,
        });
    }
    if (baseline.confidence - candidate.confidence).abs() > policy.confidence_tolerance {
        changes.push(StepChange::Confidence {
            baseline: baseline.confidence,
            candidate: candidate.confidence,
        });
    }
    if baseline.error != candidate.error {
        changes.push(StepChange::Error {
            baseline: baseline.error.clone(),
            candidate: candidate.error.clone(),
        });
    }
    if policy.compare_outputs {
        let baseline_output = baseline
            .output
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        let candidate_output = candidate
            .output
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        if baseline_output != candidate_output {
            changes.push(StepChange::Output {
                baseline: baseline_output,
                candidate: candidate_output,
            });
        }
    }
    Ok(changes)
}

fn subtree_all_ok<T>(trace: &Crux<T>) -> bool {
    trace.steps.iter().all(Step::is_ok) && trace.children.iter().all(subtree_all_ok)
}

fn validate_regression_policy(policy: &RegressionPolicy) -> Result<(), EvalConfigError> {
    validate_structural_policy(&policy.structure)?;
    let mut ids = HashSet::new();
    for invariant in &policy.invariants {
        if invariant.id.is_empty() {
            return Err(EvalConfigError::InvalidInvariant {
                id: invariant.id.clone(),
                message: "id must not be empty".into(),
            });
        }
        if !ids.insert(invariant.id.clone()) {
            return Err(EvalConfigError::DuplicateInvariantId(invariant.id.clone()));
        }
        if let Some(confidence) = invariant.minimum_confidence
            && (!confidence.is_finite() || !(0.0..=1.0).contains(&confidence))
        {
            return Err(EvalConfigError::InvalidInvariant {
                id: invariant.id.clone(),
                message: "minimum_confidence must be finite and between 0 and 1".into(),
            });
        }
    }
    Ok(())
}

fn evaluate_invariants<T>(
    candidate: &Crux<T>,
    invariants: &[StepInvariant],
) -> Vec<InvariantViolation> {
    invariants
        .iter()
        .filter_map(|invariant| evaluate_invariant(candidate, invariant).err())
        .collect()
}

fn evaluate_invariant<T>(
    candidate: &Crux<T>,
    invariant: &StepInvariant,
) -> Result<(), InvariantViolation> {
    let passed = if invariant.selector.trace_path.0.is_empty() {
        invariant_steps_pass(&candidate.steps, invariant)
    } else if let Some(trace) = child_trace_at_path(candidate, &invariant.selector.trace_path) {
        invariant_steps_pass(&trace.steps, invariant)
    } else {
        return Err(invariant_violation(
            invariant,
            "selected trace path does not exist",
        ));
    };
    if passed {
        Ok(())
    } else {
        Err(invariant_violation(
            invariant,
            "selected steps did not satisfy status and confidence requirements",
        ))
    }
}

fn invariant_steps_pass<T>(steps: &[Step<T>], invariant: &StepInvariant) -> bool {
    let matching = steps
        .iter()
        .filter(|step| step_identity(step) == invariant.selector.identity)
        .collect::<Vec<_>>();
    let satisfies = |step: &Step<T>| {
        step.status == invariant.expected_status
            && invariant
                .minimum_confidence
                .is_none_or(|minimum| step.confidence >= minimum)
    };

    match invariant.selector.occurrence {
        StepOccurrence::ExactlyOne => matching.len() == 1 && satisfies(matching[0]),
        StepOccurrence::AtLeastOne => matching.iter().copied().any(satisfies),
        StepOccurrence::All => !matching.is_empty() && matching.iter().copied().all(satisfies),
        StepOccurrence::Nth(index) => matching.get(index).is_some_and(|step| satisfies(step)),
    }
}

fn invariant_violation(invariant: &StepInvariant, message: &str) -> InvariantViolation {
    InvariantViolation {
        invariant_id: invariant.id.clone(),
        selector: invariant.selector.clone(),
        message: format!("invariant '{}': {message}", invariant.id),
    }
}

fn child_trace_at_path<'a, T>(
    trace: &'a Crux<T>,
    path: &TracePath,
) -> Option<&'a Crux<serde_json::Value>> {
    let (first, rest) = path.0.split_first()?;
    rest.iter()
        .try_fold(trace.children.get(*first)?, |current, index| {
            current.children.get(*index)
        })
}

fn compare_step_fields<T: Serialize>(
    baseline: &Step<T>,
    candidate: &Step<T>,
    policy: &StructuralPolicy,
) -> Result<Vec<StepChange>, EvalConfigError> {
    let mut changes = Vec::new();
    if baseline.kind != candidate.kind {
        changes.push(StepChange::Kind {
            baseline: baseline.kind,
            candidate: candidate.kind,
        });
    }
    if baseline.status != candidate.status {
        changes.push(StepChange::Status {
            baseline: baseline.status,
            candidate: candidate.status,
        });
    }
    if (baseline.confidence - candidate.confidence).abs() > policy.confidence_tolerance {
        changes.push(StepChange::Confidence {
            baseline: baseline.confidence,
            candidate: candidate.confidence,
        });
    }
    if baseline.input_hash != candidate.input_hash {
        changes.push(StepChange::InputHash {
            baseline: baseline.input_hash,
            candidate: candidate.input_hash,
        });
    }
    if baseline.content_hash != candidate.content_hash {
        changes.push(StepChange::ContentHash {
            baseline: baseline.content_hash,
            candidate: candidate.content_hash,
        });
    }
    if policy.compare_outputs {
        let baseline_output = baseline
            .output
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        let candidate_output = candidate
            .output
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?;
        if baseline_output != candidate_output {
            changes.push(StepChange::Output {
                baseline: baseline_output,
                candidate: candidate_output,
            });
        }
    }
    if baseline.error != candidate.error {
        changes.push(StepChange::Error {
            baseline: baseline.error.clone(),
            candidate: candidate.error.clone(),
        });
    }
    if baseline.attempt != candidate.attempt {
        changes.push(StepChange::Attempt {
            baseline: baseline.attempt,
            candidate: candidate.attempt,
        });
    }
    Ok(changes)
}

fn record_subtree<T>(
    trace: &Crux<T>,
    path: &TracePath,
    change: StepChange,
    diffs: &mut Vec<StepDiff>,
) where
    StepChange: Clone,
{
    for (index, step) in trace.steps.iter().enumerate() {
        let (baseline_index, candidate_index) = match change {
            StepChange::Removed => (Some(index), None),
            StepChange::Added => (None, Some(index)),
            _ => (None, None),
        };
        diffs.push(StepDiff {
            trace_path: path.clone(),
            baseline_index,
            candidate_index,
            identity: step_identity(step),
            changes: vec![change.clone()],
        });
    }
    for (index, child) in trace.children.iter().enumerate() {
        let mut child_path = path.0.clone();
        child_path.push(index);
        record_subtree(child, &TracePath(child_path), change.clone(), diffs);
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
    /// Rejects strategy changes that violate policy limits.
    fn validate_strategy(&self, diff: &StrategyDiff) -> Result<(), StrategyViolation>;
    /// Reports whether a strategy change requires explicit approval.
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

#[cfg(test)]
mod tests {
    use proptest::{prop_assert, proptest};

    use super::*;

    fn empty_trace() -> Crux<serde_json::Value> {
        Crux {
            id: CruxId::new(),
            agent: "test".into(),
            pipeline_version: None,
            value: Ok(serde_json::json!({})),
            steps: vec![],
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        }
    }

    fn step(name: &str, status: StepStatus, confidence: f32) -> Step {
        Step {
            stable_id: None,
            name: name.into(),
            kind: StepKind::Plain,
            status,
            origin: StepOrigin::Live,
            confidence,
            started_at: Utc::now(),
            duration_ms: 100,
            input_hash: 0,
            content_hash: None,
            output: None,
            error: None,
            cited_reason: None,
            attempt: 1,
            events: vec![],
            event_subscribers: Default::default(),
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
    fn eval_harness_enforces_quality_latency_and_golden_answer() {
        let mut baseline = empty_trace();
        baseline.steps.push(step("answer", StepStatus::Ok, 0.8));
        baseline.value = Ok(serde_json::json!({"answer": 42}));
        let mut candidate = baseline.clone();
        candidate.steps[0].duration_ms = 150;
        candidate.value = Ok(serde_json::json!({"answer": 41}));

        let harness = EvalHarness::new(EvalThresholds {
            min_score_delta: -0.01,
            max_latency_ratio: 1.2,
        })
        .with_golden_answer(serde_json::json!({"answer": 42}));
        let report = harness.evaluate(&baseline, &candidate);

        assert!(!report.passed);
        assert!(
            report
                .failures
                .iter()
                .any(|failure| failure.contains("latency"))
        );
        assert!(
            report
                .failures
                .iter()
                .any(|failure| failure.contains("golden"))
        );
    }

    #[test]
    fn eval_report_serializes_unbounded_latency_without_null() {
        let baseline = empty_trace();
        let mut candidate = empty_trace();
        candidate.steps.push(step("candidate", StepStatus::Ok, 1.0));
        let harness = EvalHarness::new(EvalThresholds {
            min_score_delta: -1.0,
            max_latency_ratio: 2.0,
        });

        let report = harness.evaluate(&baseline, &candidate);
        let json = serde_json::to_value(report).expect("report must serialize");

        assert_eq!(
            json.pointer("/latency/ratio/kind"),
            Some(&serde_json::json!("unbounded"))
        );
    }

    #[test]
    fn eval_harness_rejects_non_finite_thresholds() {
        for thresholds in [
            EvalThresholds {
                min_score_delta: f32::NAN,
                max_latency_ratio: 1.0,
            },
            EvalThresholds {
                min_score_delta: 0.0,
                max_latency_ratio: f32::INFINITY,
            },
        ] {
            assert!(matches!(
                EvalHarness::try_new(thresholds),
                Err(EvalConfigError::NonFiniteThreshold { .. })
            ));
        }
        assert!(matches!(
            EvalHarness::try_new(EvalThresholds {
                min_score_delta: 0.0,
                max_latency_ratio: -0.1,
            }),
            Err(EvalConfigError::NegativeLatencyRatio(_))
        ));
    }

    #[test]
    fn eval_harness_reports_finite_zero_latency() {
        let trace = empty_trace();
        let harness = EvalHarness::new(EvalThresholds {
            min_score_delta: 0.0,
            max_latency_ratio: 1.0,
        });

        let report = harness.evaluate(&trace, &trace);

        assert_eq!(report.latency.ratio, LatencyRatio::Finite(1.0));
        assert!(report.passed);
    }

    #[test]
    fn eval_report_round_trips_unbounded_latency() {
        let baseline = empty_trace();
        let mut candidate = empty_trace();
        candidate.steps.push(step("candidate", StepStatus::Ok, 1.0));
        let harness = EvalHarness::new(EvalThresholds {
            min_score_delta: -1.0,
            max_latency_ratio: 2.0,
        });

        let report = harness.evaluate(&baseline, &candidate);
        let json = serde_json::to_string(&report).expect("report must serialize");
        let restored: EvalReport = serde_json::from_str(&json).expect("report must deserialize");

        assert_eq!(restored.latency.ratio, LatencyRatio::Unbounded);
        assert!(!restored.passed);
    }

    #[test]
    fn eval_report_deserializes_legacy_latency_ratio() {
        let trace = empty_trace();
        let report = EvalHarness::new(EvalThresholds {
            min_score_delta: 0.0,
            max_latency_ratio: 1.0,
        })
        .evaluate(&trace, &trace);
        let mut json = serde_json::to_value(report).unwrap();
        json.as_object_mut().unwrap().remove("latency");

        let restored: EvalReport = serde_json::from_value(json).unwrap();

        assert_eq!(restored.latency_ratio, 1.0);
        assert_eq!(restored.latency, LatencyComparison::default());
    }

    fn strict_policy() -> StructuralPolicy {
        StructuralPolicy {
            mode: TraceDiffMode::Strict,
            confidence_tolerance: 0.0,
            compare_outputs: true,
            candidate_trace: CandidateTracePolicy::LiveOnly,
        }
    }

    #[test]
    fn strict_diff_is_reflexive() {
        let mut trace = empty_trace();
        trace.steps.push(step("stable", StepStatus::Ok, 0.9));

        let diff = diff_traces(&trace, &trace, &strict_policy()).expect("policy is valid");

        assert!(diff.matches);
        assert!(diff.step_diffs.is_empty());
    }

    #[test]
    fn strict_diff_reports_identity_drift() {
        let mut baseline = empty_trace();
        baseline.steps.push(step("baseline", StepStatus::Ok, 0.9));
        let mut candidate = baseline.clone();
        candidate.steps[0].name = "candidate".into();

        let diff = diff_traces(&baseline, &candidate, &strict_policy()).expect("policy is valid");

        assert!(!diff.matches);
        assert_eq!(diff.step_diffs.len(), 2);
        assert!(
            diff.step_diffs
                .iter()
                .any(|step| step.changes == vec![StepChange::Removed])
        );
        assert!(
            diff.step_diffs
                .iter()
                .any(|step| step.changes == vec![StepChange::Added])
        );
    }

    #[test]
    fn strict_diff_uses_confidence_tolerance() {
        let mut baseline = empty_trace();
        baseline.steps.push(step("stable", StepStatus::Ok, 0.9));
        let mut candidate = baseline.clone();
        candidate.steps[0].confidence = 0.85;
        let mut policy = strict_policy();
        policy.confidence_tolerance = 0.051;

        assert!(
            diff_traces(&baseline, &candidate, &policy)
                .expect("policy is valid")
                .matches
        );

        policy.confidence_tolerance = 0.049;
        assert!(
            !diff_traces(&baseline, &candidate, &policy)
                .expect("policy is valid")
                .matches
        );
    }

    #[test]
    fn strict_diff_recurses_through_serialized_children() {
        let mut baseline = empty_trace();
        let mut child = empty_trace();
        child.steps.push(step("child", StepStatus::Ok, 1.0));
        baseline.children.push(child);
        let mut candidate = baseline.clone();
        candidate.children[0].steps[0].status = StepStatus::Err;

        let diff = diff_traces(&baseline, &candidate, &strict_policy()).expect("policy is valid");

        assert!(!diff.matches);
        assert_eq!(diff.step_diffs[0].trace_path, TracePath(vec![0]));
        assert!(matches!(
            diff.step_diffs[0].changes.as_slice(),
            [StepChange::Status { .. }]
        ));
    }

    #[test]
    fn structural_policy_rejects_invalid_tolerance() {
        let trace = empty_trace();
        for tolerance in [-0.1, f32::NAN] {
            let mut policy = strict_policy();
            policy.confidence_tolerance = tolerance;
            assert!(diff_traces(&trace, &trace, &policy).is_err());
        }
    }

    #[test]
    fn strict_diff_rejects_replayed_candidate_by_default() {
        let mut baseline = empty_trace();
        baseline.steps.push(step("stable", StepStatus::Ok, 1.0));
        let mut candidate = baseline.clone();
        candidate.steps[0].origin = StepOrigin::Replayed;

        assert!(matches!(
            diff_traces(&baseline, &candidate, &strict_policy()),
            Err(EvalConfigError::ReplayedCandidateStep { .. })
        ));
    }

    fn compatible_policy() -> StructuralPolicy {
        StructuralPolicy {
            mode: TraceDiffMode::Compatible,
            confidence_tolerance: 0.0,
            compare_outputs: false,
            candidate_trace: CandidateTracePolicy::LiveOnly,
        }
    }

    #[test]
    fn compatible_diff_allows_successful_insertions() {
        let mut baseline = empty_trace();
        baseline.steps.push(step("first", StepStatus::Ok, 1.0));
        let mut candidate = baseline.clone();
        candidate
            .steps
            .insert(0, step("added", StepStatus::Ok, 1.0));

        let diff =
            diff_traces(&baseline, &candidate, &compatible_policy()).expect("policy is valid");

        assert!(diff.matches);
        assert!(
            diff.step_diffs
                .iter()
                .any(|step| step.changes == vec![StepChange::Added])
        );
    }

    #[test]
    fn compatible_diff_rejects_removed_or_reordered_baseline_steps() {
        let mut baseline = empty_trace();
        baseline.steps = vec![
            step("first", StepStatus::Ok, 1.0),
            step("second", StepStatus::Ok, 1.0),
        ];
        let mut candidate = baseline.clone();
        candidate.steps.swap(0, 1);

        let diff =
            diff_traces(&baseline, &candidate, &compatible_policy()).expect("policy is valid");

        assert!(!diff.matches);
    }

    #[test]
    fn compatible_diff_rejects_added_non_ok_steps() {
        let baseline = empty_trace();
        let mut candidate = baseline.clone();
        candidate
            .steps
            .push(step("failed-addition", StepStatus::Err, 0.0));

        let diff =
            diff_traces(&baseline, &candidate, &compatible_policy()).expect("policy is valid");

        assert!(!diff.matches);
    }

    #[test]
    fn compatible_diff_rejects_new_error_payload() {
        let mut baseline = empty_trace();
        baseline.steps.push(step("stable", StepStatus::Ok, 1.0));
        let mut candidate = baseline.clone();
        candidate.steps[0].error = Some("unexpected warning".into());

        let diff =
            diff_traces(&baseline, &candidate, &compatible_policy()).expect("policy is valid");

        assert!(!diff.matches);
        assert!(matches!(
            diff.step_diffs[0].changes.as_slice(),
            [StepChange::Error { .. }]
        ));
    }

    #[test]
    fn structural_diff_rejects_invalid_step_confidence() {
        let mut trace = empty_trace();
        trace.steps.push(step("invalid", StepStatus::Ok, f32::NAN));

        assert!(matches!(
            diff_traces(&trace, &trace, &strict_policy()),
            Err(EvalConfigError::InvalidStepConfidence { .. })
        ));
    }

    #[test]
    fn structural_diff_can_explicitly_allow_replayed_candidates() {
        let mut baseline = empty_trace();
        baseline.steps.push(step("stable", StepStatus::Ok, 1.0));
        let mut candidate = baseline.clone();
        candidate.steps[0].origin = StepOrigin::Replayed;
        let mut policy = strict_policy();
        policy.candidate_trace = CandidateTracePolicy::AllowReplayed;

        assert!(
            diff_traces(&baseline, &candidate, &policy)
                .expect("policy is valid")
                .matches
        );
    }

    #[test]
    fn step_selector_distinguishes_trace_path_and_occurrence() {
        let baseline = empty_trace();
        let mut candidate = empty_trace();
        let mut child = empty_trace();
        child.steps = vec![
            step("gate", StepStatus::Ok, 0.5),
            step("gate", StepStatus::Rejected, 1.0),
        ];
        candidate.children.push(child);
        let policy = RegressionPolicy {
            metrics: EvalThresholds {
                min_score_delta: -1.0,
                max_latency_ratio: 10.0,
            },
            structure: StructuralPolicy {
                mode: TraceDiffMode::Compatible,
                confidence_tolerance: 1.0,
                compare_outputs: false,
                candidate_trace: CandidateTracePolicy::LiveOnly,
            },
            expected_output: None,
            invariants: vec![StepInvariant {
                id: "second-child-gate".into(),
                selector: StepSelector {
                    trace_path: TracePath(vec![0]),
                    identity: StepIdentity::Name("gate".into()),
                    occurrence: StepOccurrence::Nth(1),
                },
                expected_status: StepStatus::Rejected,
                minimum_confidence: Some(0.9),
            }],
        };

        let evaluation =
            evaluate_regression(&baseline, &candidate, &policy).expect("policy is valid");

        assert!(evaluation.invariant_violations.is_empty());
    }

    #[test]
    fn invariants_collect_all_violations_in_policy_order() {
        let trace = empty_trace();
        let invariant = |id: &str| StepInvariant {
            id: id.into(),
            selector: StepSelector {
                trace_path: TracePath(vec![]),
                identity: StepIdentity::Name("missing".into()),
                occurrence: StepOccurrence::ExactlyOne,
            },
            expected_status: StepStatus::Ok,
            minimum_confidence: None,
        };

        let violations = evaluate_invariants(&trace, &[invariant("first"), invariant("second")]);

        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0].invariant_id, "first");
        assert_eq!(violations[1].invariant_id, "second");
    }

    #[test]
    fn regression_policy_rejects_duplicate_invariant_ids() {
        let trace = empty_trace();
        let invariant = StepInvariant {
            id: "duplicate".into(),
            selector: StepSelector {
                trace_path: TracePath(vec![]),
                identity: StepIdentity::Name("missing".into()),
                occurrence: StepOccurrence::AtLeastOne,
            },
            expected_status: StepStatus::Ok,
            minimum_confidence: None,
        };
        let policy = RegressionPolicy {
            metrics: EvalThresholds {
                min_score_delta: 0.0,
                max_latency_ratio: 1.0,
            },
            structure: strict_policy(),
            expected_output: None,
            invariants: vec![invariant.clone(), invariant],
        };

        assert!(matches!(
            evaluate_regression(&trace, &trace, &policy),
            Err(EvalConfigError::DuplicateInvariantId(id)) if id == "duplicate"
        ));
    }

    proptest! {
        #[test]
        fn strict_diff_is_reflexive_for_valid_confidence(confidence in 0.0_f32..=1.0) {
            let mut trace = empty_trace();
            trace.steps.push(step("generated", StepStatus::Ok, confidence));

            let diff = diff_traces(&trace, &trace, &strict_policy()).unwrap();

            prop_assert!(diff.matches);
            prop_assert!(diff.step_diffs.is_empty());
        }
    }

    #[test]
    fn regression_evaluation_composes_all_failure_classes() {
        let mut baseline = empty_trace();
        baseline.steps.push(step("safety", StepStatus::Ok, 1.0));
        let mut candidate = baseline.clone();
        candidate.steps[0].status = StepStatus::Err;
        candidate.steps[0].confidence = 0.1;
        let policy = RegressionPolicy {
            metrics: EvalThresholds {
                min_score_delta: 0.0,
                max_latency_ratio: 1.0,
            },
            structure: strict_policy(),
            expected_output: Some(serde_json::json!({"answer": 42})),
            invariants: vec![StepInvariant {
                id: "safety-gate".into(),
                selector: StepSelector {
                    trace_path: TracePath(vec![]),
                    identity: StepIdentity::Name("safety".into()),
                    occurrence: StepOccurrence::ExactlyOne,
                },
                expected_status: StepStatus::Rejected,
                minimum_confidence: Some(0.9),
            }],
        };

        let evaluation =
            evaluate_regression(&baseline, &candidate, &policy).expect("policy is valid");

        assert!(!evaluation.passed);
        assert!(!evaluation.metrics.passed);
        assert!(!evaluation.structure.matches);
        assert_eq!(evaluation.invariant_violations.len(), 1);
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
