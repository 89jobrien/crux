use std::collections::BTreeMap;

use chrono::Utc;
use crux_improve::{
    Crux, RegressionPolicy, StructuralPolicy, TraceDiff, diff_traces, evaluate_regression,
};
use serde_json::Value;

use crate::{
    BaselineRef, RegressionCaseId, RegressionError, RegressionReport, RegressionRunId,
    RegressionStore, TraceDigest,
};

/// Orchestrates baseline lifecycle and persisted regression evaluation.
#[derive(Debug)]
pub struct RegressionHarness<S> {
    store: S,
}

impl<S: RegressionStore> RegressionHarness<S> {
    /// Creates a harness backed by the supplied store adapter.
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Borrows the configured store adapter.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Stores an immutable trace artifact.
    pub fn ingest(&self, trace: &Crux<Value>) -> Result<TraceDigest, RegressionError> {
        self.store.put_trace(trace)
    }

    /// Promotes a stored artifact with compare-and-set protection.
    ///
    /// # Errors
    ///
    /// Returns an error when the artifact is missing or the expected current digest is stale.
    pub fn promote(
        &self,
        case: RegressionCaseId,
        digest: TraceDigest,
        expected: Option<TraceDigest>,
        labels: BTreeMap<String, String>,
    ) -> Result<BaselineRef, RegressionError> {
        self.store.trace(&digest)?;
        let baseline = BaselineRef {
            case: case.clone(),
            trace: digest,
            labels,
            promoted_at: Utc::now(),
        };
        self.store
            .compare_and_set_baseline(&case, expected.as_ref(), &baseline)?;
        Ok(baseline)
    }

    /// Evaluates a candidate and appends its report before returning.
    ///
    /// # Errors
    ///
    /// Returns an error for missing baselines, corrupt artifacts, invalid policies, or failed
    /// report persistence.
    pub fn evaluate(
        &self,
        case: &RegressionCaseId,
        candidate: &Crux<Value>,
        policy: &RegressionPolicy,
    ) -> Result<RegressionReport, RegressionError> {
        let baseline_ref = self.store.baseline(case)?;
        let baseline = self.store.trace(&baseline_ref.trace)?;
        let candidate_digest = self.store.put_trace(candidate)?;
        let evaluation = evaluate_regression(&baseline, candidate, policy)?;
        let report = RegressionReport {
            id: RegressionRunId::new(),
            case: case.clone(),
            baseline: baseline_ref.trace,
            candidate: candidate_digest,
            policy: policy.clone(),
            evaluation,
            created_at: Utc::now(),
        };
        self.store.append_report(&report)?;
        Ok(report)
    }

    /// Compares candidate structure without persisting the candidate or a report.
    pub fn diff(
        &self,
        case: &RegressionCaseId,
        candidate: &Crux<Value>,
        policy: &StructuralPolicy,
    ) -> Result<TraceDiff, RegressionError> {
        let baseline_ref = self.store.baseline(case)?;
        let baseline = self.store.trace(&baseline_ref.trace)?;
        diff_traces(&baseline, candidate, policy).map_err(RegressionError::from)
    }

    /// Returns append-only report history in newest-first order.
    pub fn history(
        &self,
        case: &RegressionCaseId,
    ) -> Result<Vec<RegressionReport>, RegressionError> {
        self.store.reports(case)
    }
}
