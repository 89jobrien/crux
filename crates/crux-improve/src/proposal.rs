//! Persisted lifecycle for improvement proposals.
//!
//! A [`ProposalLifecycle`] captures everything needed to track an improvement
//! from creation through policy validation, approval gating, replay comparison,
//! acceptance, and potential rollback.
//!
//! ## State machine
//!
//! ```text
//! Draft
//!   └─► ValidationFailed        (terminal — policy rejected the diff)
//!   └─► ValidationPassed
//!         └─► AwaitingApproval  (when approval is required)
//!         └─► Accepted          (when no approval required and comparison passes)
//!         └─► Rejected          (replay comparison regressed)
//! AwaitingApproval
//!   └─► ApprovalRejected        (terminal — gatekeeper declined)
//!   └─► Accepted                (gatekeeper approved)
//! Accepted
//!   └─► RolledBack              (regression detected after acceptance)
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    Comparison, Crux, CruxId, Improvement, RegressionEvaluation, RegressionPolicy, replay_compare,
};

// ---------------------------------------------------------------------------
// PersistedRun
// ---------------------------------------------------------------------------

/// A stored execution trace with identity metadata.
///
/// The output type is erased to [`serde_json::Value`] so traces can be
/// persisted and loaded without knowing the original generic parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedRun {
    pub id: CruxId,
    pub agent: String,
    pub recorded_at: DateTime<Utc>,
    /// Type-erased execution trace.
    pub trace: Crux<serde_json::Value>,
}

impl PersistedRun {
    /// Creates a new [`PersistedRun`] from a typed trace, serializing its
    /// `value` field to [`serde_json::Value`].
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if the trace `value` cannot be
    /// serialized.
    pub fn from_trace<T: serde::Serialize>(trace: &Crux<T>) -> Result<Self, serde_json::Error> {
        let erased = erase_trace(trace)?;
        Ok(Self {
            id: CruxId::new(),
            agent: trace.agent.clone(),
            recorded_at: Utc::now(),
            trace: erased,
        })
    }
}

/// Erases the output type of a `Crux<T>` to `Crux<serde_json::Value>`.
///
/// `steps` and `children` are already `serde_json::Value`-typed in the `Crux`
/// definition; only `value: Result<T, CruxErr>` requires conversion.
fn erase_trace<T: serde::Serialize>(
    trace: &Crux<T>,
) -> Result<Crux<serde_json::Value>, serde_json::Error> {
    // Let the compiler infer the Result error type from context.
    let value = match &trace.value {
        Ok(v) => Ok(serde_json::to_value(v)?),
        Err(e) => Err(e.clone()),
    };
    Ok(Crux {
        id: trace.id.clone(),
        agent: trace.agent.clone(),
        pipeline_version: trace.pipeline_version.clone(),
        value,
        steps: trace.steps.clone(),
        children: trace.children.clone(),
        started_at: trace.started_at,
        finished_at: trace.finished_at,
    })
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Current state of a proposal in its lifecycle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProposalStatus {
    /// Proposal created; policy validation not yet run.
    Draft,
    /// Policy validation ran and found violations.
    ValidationFailed { violations: Vec<String> },
    /// Policy validation passed; no approval required, but replay comparison
    /// has not been recorded yet.
    ValidationPassed,
    /// Approval is required; waiting for a gatekeeper decision.
    AwaitingApproval,
    /// Gatekeeper explicitly rejected the proposal.
    ApprovalRejected {
        rejected_by: String,
        rejected_at: DateTime<Utc>,
        reason: Option<String>,
    },
    /// Proposal accepted (either directly after validation or after approval).
    Accepted { accepted_at: DateTime<Utc> },
    /// Accepted proposal was later rolled back.
    RolledBack {
        rolled_back_at: DateTime<Utc>,
        reason: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Sub-records
// ---------------------------------------------------------------------------

/// Result of running the [`StrategyPolicy`] against a proposal's diff.
///
/// [`StrategyPolicy`]: crate::StrategyPolicy
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyValidationRecord {
    pub validated_at: DateTime<Utc>,
    pub passed: bool,
    /// Human-readable violation messages when `passed == false`.
    pub violations: Vec<String>,
}

/// Decision a gatekeeper rendered on a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Granted,
    Rejected,
}

/// Approval gate record: whether approval was required, and the outcome if so.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    /// Whether the policy decided that explicit approval was required.
    pub required: bool,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision: Option<ApprovalDecision>,
    pub decided_by: Option<String>,
    pub reason: Option<String>,
}

/// One event in the immutable audit history of a proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HistoryEvent {
    Created {
        at: DateTime<Utc>,
    },
    ValidationRecorded {
        at: DateTime<Utc>,
        passed: bool,
        violations: Vec<String>,
    },
    ApprovalRequired {
        at: DateTime<Utc>,
    },
    ApprovalDecided {
        at: DateTime<Utc>,
        decision: ApprovalDecision,
        by: String,
        reason: Option<String>,
    },
    ReplayComparisonRecorded {
        at: DateTime<Utc>,
    },
    Accepted {
        at: DateTime<Utc>,
    },
    RolledBack {
        at: DateTime<Utc>,
        reason: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Invalid lifecycle transition.
#[derive(Debug, Error, PartialEq)]
pub enum ProposalTransitionError {
    #[error("cannot record policy validation: proposal is not in Draft state (current: {current})")]
    NotDraft { current: &'static str },

    #[error("cannot request approval: proposal has not passed validation (current: {current})")]
    ValidationNotPassed { current: &'static str },

    #[error("cannot decide approval: proposal is not awaiting approval (current: {current})")]
    NotAwaitingApproval { current: &'static str },

    #[error(
        "cannot accept: proposal must be in ValidationPassed or ApprovalGranted state (current: {current})"
    )]
    CannotAccept { current: &'static str },

    #[error("cannot roll back: proposal is not in Accepted state (current: {current})")]
    NotAccepted { current: &'static str },

    #[error(
        "replay comparison requires an accepted or validation-passed proposal (current: {current})"
    )]
    CannotRecordComparison { current: &'static str },

    #[error("baseline run must be attached before comparing")]
    MissingBaselineRun,

    #[error("candidate run must be attached before comparing")]
    MissingCandidateRun,

    #[error("run comparison failed: {0}")]
    ComparisonFailed(String),
}

// ---------------------------------------------------------------------------
// ProposalLifecycle
// ---------------------------------------------------------------------------

/// Full persisted lifecycle record for one improvement proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalLifecycle {
    pub id: CruxId,
    pub improvement: Improvement,
    pub status: ProposalStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Result of policy validation, populated after the first call to
    /// [`record_policy_validation`](ProposalLifecycle::record_policy_validation).
    pub policy_validation: Option<PolicyValidationRecord>,
    /// Approval gate record, populated when approval is required or decided.
    pub approval: Option<ApprovalRecord>,
    /// Output of the replay comparison stored after the run completes.
    pub replay_comparison: Option<Comparison>,
    /// Baseline persisted run attached for comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_run: Option<PersistedRun>,
    /// Candidate persisted run attached for comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_run: Option<PersistedRun>,
    /// Immutable append-only audit history.
    pub history: Vec<HistoryEvent>,
}

impl ProposalLifecycle {
    /// Creates a new proposal in [`ProposalStatus::Draft`] state.
    pub fn new(improvement: Improvement) -> Self {
        let now = Utc::now();
        let id = improvement.id.clone();
        Self {
            id,
            improvement,
            status: ProposalStatus::Draft,
            created_at: now,
            updated_at: now,
            policy_validation: None,
            approval: None,
            replay_comparison: None,
            baseline_run: None,
            candidate_run: None,
            history: vec![HistoryEvent::Created { at: now }],
        }
    }

    /// Returns a short static label for the current status (for error messages).
    fn status_label(&self) -> &'static str {
        match &self.status {
            ProposalStatus::Draft => "Draft",
            ProposalStatus::ValidationFailed { .. } => "ValidationFailed",
            ProposalStatus::ValidationPassed => "ValidationPassed",
            ProposalStatus::AwaitingApproval => "AwaitingApproval",
            ProposalStatus::ApprovalRejected { .. } => "ApprovalRejected",
            ProposalStatus::Accepted { .. } => "Accepted",
            ProposalStatus::RolledBack { .. } => "RolledBack",
        }
    }

    /// Records the result of running the strategy policy against this proposal.
    ///
    /// Transitions:
    /// - `Draft` → `ValidationFailed` when violations are non-empty
    /// - `Draft` → `ValidationPassed` when validation passes
    ///
    /// # Errors
    ///
    /// Returns [`ProposalTransitionError::NotDraft`] if the proposal is not in
    /// `Draft` state.
    pub fn record_policy_validation(
        &mut self,
        passed: bool,
        violations: Vec<String>,
    ) -> Result<(), ProposalTransitionError> {
        if !matches!(self.status, ProposalStatus::Draft) {
            return Err(ProposalTransitionError::NotDraft {
                current: self.status_label(),
            });
        }
        let now = Utc::now();
        let record = PolicyValidationRecord {
            validated_at: now,
            passed,
            violations: violations.clone(),
        };
        self.history.push(HistoryEvent::ValidationRecorded {
            at: now,
            passed,
            violations: violations.clone(),
        });
        self.status = if passed {
            ProposalStatus::ValidationPassed
        } else {
            ProposalStatus::ValidationFailed { violations }
        };
        self.policy_validation = Some(record);
        self.updated_at = now;
        Ok(())
    }

    /// Marks the proposal as requiring explicit approval.
    ///
    /// Transitions: `ValidationPassed` → `AwaitingApproval`
    ///
    /// # Errors
    ///
    /// Returns [`ProposalTransitionError::ValidationNotPassed`] if the proposal
    /// has not completed validation successfully.
    pub fn request_approval(&mut self) -> Result<(), ProposalTransitionError> {
        if !matches!(self.status, ProposalStatus::ValidationPassed) {
            return Err(ProposalTransitionError::ValidationNotPassed {
                current: self.status_label(),
            });
        }
        let now = Utc::now();
        self.approval = Some(ApprovalRecord {
            required: true,
            decided_at: None,
            decision: None,
            decided_by: None,
            reason: None,
        });
        self.history
            .push(HistoryEvent::ApprovalRequired { at: now });
        self.status = ProposalStatus::AwaitingApproval;
        self.updated_at = now;
        Ok(())
    }

    /// Records a gatekeeper decision on the approval gate.
    ///
    /// Transitions:
    /// - `AwaitingApproval` → `Accepted` when decision is `Granted`
    /// - `AwaitingApproval` → `ApprovalRejected` when decision is `Rejected`
    ///
    /// # Errors
    ///
    /// Returns [`ProposalTransitionError::NotAwaitingApproval`] if the proposal
    /// is not in `AwaitingApproval` state.
    pub fn decide_approval(
        &mut self,
        decision: ApprovalDecision,
        decided_by: impl Into<String>,
        reason: Option<String>,
    ) -> Result<(), ProposalTransitionError> {
        if !matches!(self.status, ProposalStatus::AwaitingApproval) {
            return Err(ProposalTransitionError::NotAwaitingApproval {
                current: self.status_label(),
            });
        }
        let now = Utc::now();
        let by = decided_by.into();
        if let Some(record) = self.approval.as_mut() {
            record.decided_at = Some(now);
            record.decision = Some(decision);
            record.decided_by = Some(by.clone());
            record.reason = reason.clone();
        }
        self.history.push(HistoryEvent::ApprovalDecided {
            at: now,
            decision,
            by: by.clone(),
            reason: reason.clone(),
        });
        self.status = match decision {
            ApprovalDecision::Granted => ProposalStatus::Accepted { accepted_at: now },
            ApprovalDecision::Rejected => ProposalStatus::ApprovalRejected {
                rejected_by: by,
                rejected_at: now,
                reason,
            },
        };
        self.updated_at = now;
        Ok(())
    }

    /// Stores the replay comparison output for this proposal.
    ///
    /// May be called from `ValidationPassed` or `Accepted` states (comparison
    /// can be run before or after formal acceptance).
    ///
    /// # Errors
    ///
    /// Returns [`ProposalTransitionError::CannotRecordComparison`] in terminal
    /// states where a comparison no longer makes sense.
    pub fn record_replay_comparison(
        &mut self,
        comparison: Comparison,
    ) -> Result<(), ProposalTransitionError> {
        match &self.status {
            ProposalStatus::ValidationPassed | ProposalStatus::Accepted { .. } => {}
            _ => {
                return Err(ProposalTransitionError::CannotRecordComparison {
                    current: self.status_label(),
                });
            }
        }
        let now = Utc::now();
        self.history
            .push(HistoryEvent::ReplayComparisonRecorded { at: now });
        self.replay_comparison = Some(comparison);
        self.updated_at = now;
        Ok(())
    }

    /// Transitions the proposal to `Accepted`.
    ///
    /// Valid from `ValidationPassed` (no approval required) or `AwaitingApproval`
    /// after a grant via [`decide_approval`](ProposalLifecycle::decide_approval).
    /// Calling [`decide_approval`] with `Granted` automatically transitions to
    /// `Accepted`, so this method is only needed for the no-approval path.
    ///
    /// # Errors
    ///
    /// Returns [`ProposalTransitionError::CannotAccept`] if the current state is
    /// not `ValidationPassed`.
    pub fn accept(&mut self) -> Result<(), ProposalTransitionError> {
        if !matches!(self.status, ProposalStatus::ValidationPassed) {
            return Err(ProposalTransitionError::CannotAccept {
                current: self.status_label(),
            });
        }
        let now = Utc::now();
        self.history.push(HistoryEvent::Accepted { at: now });
        self.status = ProposalStatus::Accepted { accepted_at: now };
        self.updated_at = now;
        Ok(())
    }

    /// Rolls back an accepted proposal, recording an optional revert reason.
    ///
    /// # Errors
    ///
    /// Returns [`ProposalTransitionError::NotAccepted`] if the proposal is not
    /// in `Accepted` state.
    pub fn rollback(&mut self, reason: Option<String>) -> Result<(), ProposalTransitionError> {
        if !matches!(self.status, ProposalStatus::Accepted { .. }) {
            return Err(ProposalTransitionError::NotAccepted {
                current: self.status_label(),
            });
        }
        let now = Utc::now();
        self.history.push(HistoryEvent::RolledBack {
            at: now,
            reason: reason.clone(),
        });
        self.status = ProposalStatus::RolledBack {
            rolled_back_at: now,
            reason,
        };
        self.updated_at = now;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Persisted run attachment and comparison
    // -----------------------------------------------------------------------

    /// Attaches a baseline persisted run.
    ///
    /// The run is stored unconditionally and can be replaced by calling this
    /// method again.  Use [`compare_runs`](Self::compare_runs) to trigger the
    /// metric comparison once both runs are attached.
    pub fn attach_baseline_run(&mut self, run: PersistedRun) {
        self.baseline_run = Some(run);
        self.updated_at = Utc::now();
    }

    /// Attaches a candidate persisted run.
    ///
    /// The run is stored unconditionally and can be replaced by calling this
    /// method again.  Use [`compare_runs`](Self::compare_runs) to trigger the
    /// metric comparison once both runs are attached.
    pub fn attach_candidate_run(&mut self, run: PersistedRun) {
        self.candidate_run = Some(run);
        self.updated_at = Utc::now();
    }

    /// Computes a metric comparison from the attached baseline and candidate
    /// runs and stores it via
    /// [`record_replay_comparison`](Self::record_replay_comparison).
    ///
    /// Both runs must be attached before calling this method.
    ///
    /// # Errors
    ///
    /// Returns [`ProposalTransitionError::MissingBaselineRun`] or
    /// [`ProposalTransitionError::MissingCandidateRun`] when the respective
    /// run has not been attached.  Propagates any
    /// [`ProposalTransitionError::CannotRecordComparison`] from the underlying
    /// state guard.
    pub fn compare_runs(&mut self) -> Result<&Comparison, ProposalTransitionError> {
        let baseline = self
            .baseline_run
            .as_ref()
            .ok_or(ProposalTransitionError::MissingBaselineRun)?;
        let candidate = self
            .candidate_run
            .as_ref()
            .ok_or(ProposalTransitionError::MissingCandidateRun)?;
        let comparison = replay_compare(&baseline.trace, &candidate.trace);
        self.record_replay_comparison(comparison)?;
        Ok(self.replay_comparison.as_ref().expect("just stored"))
    }

    /// Runs a full [`evaluate_regression`](crate::evaluate_regression) of the
    /// attached persisted runs under `policy`, stores the metric comparison,
    /// and returns the complete [`RegressionEvaluation`].
    ///
    /// Both runs must be attached before calling this method.
    ///
    /// # Errors
    ///
    /// Returns [`ProposalTransitionError::MissingBaselineRun`] or
    /// [`ProposalTransitionError::MissingCandidateRun`] when the respective
    /// run has not been attached.  Returns
    /// [`ProposalTransitionError::ComparisonFailed`] if the policy is invalid
    /// or the structural diff fails.  Propagates
    /// [`ProposalTransitionError::CannotRecordComparison`] from the state guard.
    pub fn evaluate_runs(
        &mut self,
        policy: &RegressionPolicy,
    ) -> Result<RegressionEvaluation, ProposalTransitionError> {
        let baseline = self
            .baseline_run
            .as_ref()
            .ok_or(ProposalTransitionError::MissingBaselineRun)?;
        let candidate = self
            .candidate_run
            .as_ref()
            .ok_or(ProposalTransitionError::MissingCandidateRun)?;
        let evaluation = crate::evaluate_regression(&baseline.trace, &candidate.trace, policy)
            .map_err(|e| ProposalTransitionError::ComparisonFailed(e.to_string()))?;
        self.record_replay_comparison(evaluation.metrics.comparison.clone())?;
        Ok(evaluation)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImprovementKind, StrategyDiff, Verdict};
    use chrono::Utc;

    fn make_improvement() -> Improvement {
        Improvement {
            id: CruxId::new(),
            kind: ImprovementKind::ToolPreference,
            target: "rg".into(),
            diff: StrategyDiff::default(),
            confidence: 0.9,
            evidence: vec!["run-1".into()],
            proposed_at: Utc::now(),
        }
    }

    fn make_comparison(verdict: Verdict) -> Comparison {
        use crate::TraceMetrics;
        let metrics = TraceMetrics {
            step_count: 1,
            success_rate: 1.0,
            error_count: 0,
            avg_confidence: 0.9,
            total_duration_ms: 100,
            delegation_count: 0,
            delegation_depth: 0,
            speculation_count: 0,
            speculation_hit_count: 0,
            speculation_hit_rate: 0.0,
            score: 0.8,
        };
        let delta = match verdict {
            Verdict::Improved => 0.1,
            Verdict::Regressed => -0.1,
            Verdict::Neutral => 0.0,
        };
        Comparison {
            old_metrics: metrics.clone(),
            new_metrics: metrics,
            delta,
            verdict,
        }
    }

    // --- Happy-path lifecycle -------------------------------------------------

    #[test]
    fn new_proposal_is_draft() {
        let p = ProposalLifecycle::new(make_improvement());
        assert_eq!(p.status, ProposalStatus::Draft);
        assert!(matches!(p.history[0], HistoryEvent::Created { .. }));
    }

    #[test]
    fn validation_failure_transitions_to_validation_failed() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(false, vec!["tool pref too high".into()])
            .expect("valid transition");
        assert!(matches!(p.status, ProposalStatus::ValidationFailed { .. }));
        assert!(!p.policy_validation.as_ref().unwrap().passed);
        assert_eq!(p.history.len(), 2);
    }

    #[test]
    fn validation_pass_transitions_to_validation_passed() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![])
            .expect("valid transition");
        assert_eq!(p.status, ProposalStatus::ValidationPassed);
        assert!(p.policy_validation.as_ref().unwrap().passed);
    }

    #[test]
    fn accept_directly_after_validation() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.accept().expect("valid transition");
        assert!(matches!(p.status, ProposalStatus::Accepted { .. }));
    }

    #[test]
    fn approval_chain_grant_path() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.request_approval().unwrap();
        assert_eq!(p.status, ProposalStatus::AwaitingApproval);

        p.decide_approval(ApprovalDecision::Granted, "alice", None)
            .unwrap();
        assert!(matches!(p.status, ProposalStatus::Accepted { .. }));

        let approval = p.approval.as_ref().unwrap();
        assert_eq!(approval.decision, Some(ApprovalDecision::Granted));
        assert_eq!(approval.decided_by.as_deref(), Some("alice"));
    }

    #[test]
    fn approval_chain_reject_path() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.request_approval().unwrap();
        p.decide_approval(ApprovalDecision::Rejected, "bob", Some("too risky".into()))
            .unwrap();
        assert!(matches!(p.status, ProposalStatus::ApprovalRejected { .. }));
    }

    #[test]
    fn replay_comparison_stored_after_acceptance() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.accept().unwrap();
        p.record_replay_comparison(make_comparison(Verdict::Improved))
            .unwrap();
        assert!(p.replay_comparison.is_some());
        assert_eq!(
            p.replay_comparison.as_ref().unwrap().verdict,
            Verdict::Improved
        );
    }

    #[test]
    fn replay_comparison_stored_before_acceptance() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.record_replay_comparison(make_comparison(Verdict::Neutral))
            .unwrap();
        assert!(p.replay_comparison.is_some());
    }

    #[test]
    fn rollback_accepted_proposal() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.accept().unwrap();
        p.rollback(Some("latency spiked".into())).unwrap();
        assert!(matches!(p.status, ProposalStatus::RolledBack { .. }));
        if let ProposalStatus::RolledBack { reason, .. } = &p.status {
            assert_eq!(reason.as_deref(), Some("latency spiked"));
        }
    }

    #[test]
    fn rollback_without_reason() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.accept().unwrap();
        p.rollback(None).unwrap();
        assert!(matches!(
            p.status,
            ProposalStatus::RolledBack { reason: None, .. }
        ));
    }

    #[test]
    fn history_records_full_lifecycle() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.request_approval().unwrap();
        p.decide_approval(ApprovalDecision::Granted, "alice", None)
            .unwrap();
        p.record_replay_comparison(make_comparison(Verdict::Improved))
            .unwrap();
        p.rollback(Some("revert".into())).unwrap();

        assert!(matches!(p.history[0], HistoryEvent::Created { .. }));
        assert!(matches!(
            p.history[1],
            HistoryEvent::ValidationRecorded { passed: true, .. }
        ));
        assert!(matches!(
            p.history[2],
            HistoryEvent::ApprovalRequired { .. }
        ));
        assert!(matches!(
            p.history[3],
            HistoryEvent::ApprovalDecided {
                decision: ApprovalDecision::Granted,
                ..
            }
        ));
        assert!(matches!(
            p.history[4],
            HistoryEvent::ReplayComparisonRecorded { .. }
        ));
        assert!(matches!(p.history[5], HistoryEvent::RolledBack { .. }));
    }

    // --- Transition guards ---------------------------------------------------

    #[test]
    fn validation_rejected_from_non_draft() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        let err = p.record_policy_validation(true, vec![]).unwrap_err();
        assert!(matches!(err, ProposalTransitionError::NotDraft { .. }));
    }

    #[test]
    fn request_approval_rejected_from_draft() {
        let mut p = ProposalLifecycle::new(make_improvement());
        let err = p.request_approval().unwrap_err();
        assert!(matches!(
            err,
            ProposalTransitionError::ValidationNotPassed { .. }
        ));
    }

    #[test]
    fn decide_approval_rejected_from_validation_passed() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        let err = p
            .decide_approval(ApprovalDecision::Granted, "alice", None)
            .unwrap_err();
        assert!(matches!(
            err,
            ProposalTransitionError::NotAwaitingApproval { .. }
        ));
    }

    #[test]
    fn accept_rejected_from_awaiting_approval() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.request_approval().unwrap();
        let err = p.accept().unwrap_err();
        assert!(matches!(err, ProposalTransitionError::CannotAccept { .. }));
    }

    #[test]
    fn rollback_rejected_from_draft() {
        let mut p = ProposalLifecycle::new(make_improvement());
        let err = p.rollback(None).unwrap_err();
        assert!(matches!(err, ProposalTransitionError::NotAccepted { .. }));
    }

    #[test]
    fn replay_comparison_rejected_from_draft() {
        let mut p = ProposalLifecycle::new(make_improvement());
        let err = p
            .record_replay_comparison(make_comparison(Verdict::Neutral))
            .unwrap_err();
        assert!(matches!(
            err,
            ProposalTransitionError::CannotRecordComparison { .. }
        ));
    }

    #[test]
    fn lifecycle_round_trips_via_json() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.accept().unwrap();
        p.record_replay_comparison(make_comparison(Verdict::Improved))
            .unwrap();
        p.rollback(Some("regression".into())).unwrap();

        let json = serde_json::to_string(&p).expect("serialize");
        let back: ProposalLifecycle = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, p.id);
        assert_eq!(back.status, p.status);
        assert_eq!(back.history.len(), p.history.len());
    }

    #[test]
    fn validation_failure_carries_violations_in_status_and_record() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(false, vec!["v1".into(), "v2".into()])
            .unwrap();
        if let ProposalStatus::ValidationFailed { violations } = &p.status {
            assert_eq!(violations, &["v1", "v2"]);
        } else {
            panic!("expected ValidationFailed");
        }
        let record = p.policy_validation.as_ref().unwrap();
        assert_eq!(record.violations, ["v1", "v2"]);
    }

    // --- Persisted run attachment and comparison ----------------------------

    fn empty_trace() -> crate::Crux<serde_json::Value> {
        use crate::CruxId;
        crate::Crux {
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

    fn make_run() -> PersistedRun {
        PersistedRun {
            id: crate::CruxId::new(),
            agent: "test".into(),
            recorded_at: Utc::now(),
            trace: empty_trace(),
        }
    }

    #[test]
    fn attach_runs_stores_runs() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.attach_baseline_run(make_run());
        p.attach_candidate_run(make_run());
        assert!(p.baseline_run.is_some());
        assert!(p.candidate_run.is_some());
    }

    #[test]
    fn compare_runs_requires_both_runs() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();

        assert!(matches!(
            p.compare_runs(),
            Err(ProposalTransitionError::MissingBaselineRun)
        ));

        p.attach_baseline_run(make_run());
        assert!(matches!(
            p.compare_runs(),
            Err(ProposalTransitionError::MissingCandidateRun)
        ));
    }

    #[test]
    fn compare_runs_stores_comparison_after_validation() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.attach_baseline_run(make_run());
        p.attach_candidate_run(make_run());

        p.compare_runs().expect("compare must succeed");
        assert!(p.replay_comparison.is_some());
    }

    #[test]
    fn evaluate_runs_uses_regression_policy() {
        use crate::{
            CandidateTracePolicy, EvalThresholds, RegressionPolicy, StructuralPolicy, TraceDiffMode,
        };

        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.attach_baseline_run(make_run());
        p.attach_candidate_run(make_run());

        let policy = RegressionPolicy {
            metrics: EvalThresholds {
                min_score_delta: -1.0,
                max_latency_ratio: 100.0,
            },
            structure: StructuralPolicy {
                mode: TraceDiffMode::Compatible,
                confidence_tolerance: 1.0,
                compare_outputs: false,
                candidate_trace: CandidateTracePolicy::LiveOnly,
            },
            expected_output: None,
            invariants: vec![],
        };

        let eval = p.evaluate_runs(&policy).expect("evaluate must succeed");
        assert!(eval.passed);
        assert!(p.replay_comparison.is_some());
    }

    #[test]
    fn compare_runs_integrates_with_accept_lifecycle() {
        let mut p = ProposalLifecycle::new(make_improvement());
        p.record_policy_validation(true, vec![]).unwrap();
        p.attach_baseline_run(make_run());
        p.attach_candidate_run(make_run());
        p.compare_runs().expect("compare must succeed");
        p.accept().expect("accept must succeed");

        assert!(matches!(p.status, ProposalStatus::Accepted { .. }));
        assert!(p.replay_comparison.is_some());
    }

    #[test]
    fn persisted_run_from_trace_round_trip() {
        let trace = empty_trace();
        let run = PersistedRun::from_trace(&trace).expect("from_trace must succeed");
        assert_eq!(run.agent, "test");
        assert!(run.trace.steps.is_empty());
    }
}
