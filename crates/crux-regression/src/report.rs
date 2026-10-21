use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use crux_improve::{RegressionEvaluation, RegressionPolicy};
use serde::{Deserialize, Serialize};

use crate::{RegressionCaseId, RegressionRunId, TraceDigest};

/// Mutable reference selecting the active golden trace for one case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineRef {
    /// Scenario whose active baseline is selected.
    pub case: RegressionCaseId,
    /// Immutable trace artifact selected as the baseline.
    pub trace: TraceDigest,
    /// Operator-provided baseline metadata.
    pub labels: BTreeMap<String, String>,
    /// Time at which this reference was promoted.
    pub promoted_at: DateTime<Utc>,
}

/// Append-only result of evaluating one candidate trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegressionReport {
    /// Unique append-only report identifier.
    pub id: RegressionRunId,
    /// Evaluated regression scenario.
    pub case: RegressionCaseId,
    /// Baseline artifact used for evaluation.
    pub baseline: TraceDigest,
    /// Candidate artifact used for evaluation.
    pub candidate: TraceDigest,
    /// Complete policy applied during evaluation.
    pub policy: RegressionPolicy,
    /// Deterministic evaluation result.
    pub evaluation: RegressionEvaluation,
    /// Time at which the report was created.
    pub created_at: DateTime<Utc>,
}
