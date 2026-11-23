//! Domain service that drives improvement proposals through policy validation,
//! approval, replay-based promotion, and rollback.
//!
//! [`ProposalService`] depends only on the [`ProposalStore`] port. Every
//! transition is written back through the port, so the audit history on each
//! [`ProposalLifecycle`] survives restarts when a durable adapter is injected.
//!
//! Promotion is gated on replay evidence: a proposal can only be accepted, or
//! approval can only be granted, after [`ProposalService::evaluate`] has stored
//! a passing [`RegressionEvaluation`].

use thiserror::Error;

use crate::{
    CruxId, Improvement, RegressionEvaluation, RegressionPolicy, StrategyPolicy,
    proposal::{
        ApprovalDecision, PersistedRun, ProposalLifecycle, ProposalStatus, ProposalTransitionError,
    },
    store::{ProposalStore, ProposalStoreError},
};

/// Errors produced by [`ProposalService`].
#[derive(Debug, Error)]
pub enum ProposalServiceError {
    #[error("proposal not found: {0}")]
    NotFound(String),
    #[error(transparent)]
    Store(#[from] ProposalStoreError),
    #[error(transparent)]
    Transition(#[from] ProposalTransitionError),
    #[error("no replay evaluation is stored for this proposal")]
    EvaluationMissing,
    #[error("replay evaluation failed: {}", .0.join("; "))]
    EvaluationFailed(Vec<String>),
}

/// Orchestrates the persisted proposal lifecycle over a [`ProposalStore`].
#[derive(Debug)]
pub struct ProposalService<S> {
    store: S,
}

impl<S: ProposalStore> ProposalService<S> {
    /// Wraps a store adapter.
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Returns the underlying store adapter.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Creates a `Draft` proposal for `improvement` and persists it.
    pub fn propose(
        &mut self,
        improvement: Improvement,
    ) -> Result<ProposalLifecycle, ProposalServiceError> {
        let proposal = ProposalLifecycle::new(improvement);
        self.store.create(proposal.clone())?;
        Ok(proposal)
    }

    /// Loads a proposal by id.
    pub fn get(&self, id: &CruxId) -> Result<ProposalLifecycle, ProposalServiceError> {
        self.load(id)
    }

    /// Runs `policy` against the proposal diff. A violation moves the proposal
    /// to `ValidationFailed`. A pass moves it to `ValidationPassed`, and to
    /// `AwaitingApproval` when the policy requires explicit approval.
    pub fn validate<P: StrategyPolicy>(
        &mut self,
        id: &CruxId,
        policy: &P,
    ) -> Result<ProposalLifecycle, ProposalServiceError> {
        let mut proposal = self.load(id)?;
        let violations = match policy.validate_strategy(&proposal.improvement.diff) {
            Ok(()) => Vec::new(),
            Err(violation) => vec![violation.message],
        };
        let passed = violations.is_empty();
        proposal.record_policy_validation(passed, violations)?;
        if passed && policy.requires_strategy_approval(&proposal.improvement.diff) {
            proposal.request_approval()?;
        }
        self.save(proposal)
    }

    /// Attaches the baseline and candidate runs used for replay comparison.
    ///
    /// Any previously stored regression evaluation is cleared, because it was
    /// computed against different evidence and must not gate promotion.
    pub fn attach_runs(
        &mut self,
        id: &CruxId,
        baseline: PersistedRun,
        candidate: PersistedRun,
    ) -> Result<ProposalLifecycle, ProposalServiceError> {
        let mut proposal = self.load(id)?;
        proposal.attach_baseline_run(baseline);
        proposal.attach_candidate_run(candidate);
        proposal.regression_evaluation = None;
        self.save(proposal)
    }

    /// Evaluates the attached runs under `policy`, stores the full
    /// [`RegressionEvaluation`] on the proposal, and returns it.
    pub fn evaluate(
        &mut self,
        id: &CruxId,
        policy: &RegressionPolicy,
    ) -> Result<RegressionEvaluation, ProposalServiceError> {
        let mut proposal = self.load(id)?;
        let evaluation = proposal.evaluate_runs(policy)?;
        self.save(proposal)?;
        Ok(evaluation)
    }

    /// Records a gatekeeper decision. Granting requires a stored passing
    /// evaluation; rejection does not.
    pub fn decide(
        &mut self,
        id: &CruxId,
        decision: ApprovalDecision,
        decided_by: impl Into<String>,
        reason: Option<String>,
    ) -> Result<ProposalLifecycle, ProposalServiceError> {
        let mut proposal = self.load(id)?;
        if decision == ApprovalDecision::Granted
            && matches!(proposal.status, ProposalStatus::AwaitingApproval)
        {
            Self::require_passing_evaluation(&proposal)?;
        }
        proposal.decide_approval(decision, decided_by, reason)?;
        self.save(proposal)
    }

    /// Accepts a `ValidationPassed` proposal that needs no approval. Requires a
    /// stored passing evaluation.
    pub fn promote(&mut self, id: &CruxId) -> Result<ProposalLifecycle, ProposalServiceError> {
        let mut proposal = self.load(id)?;
        Self::require_passing_evaluation(&proposal)?;
        proposal.accept()?;
        self.save(proposal)
    }

    /// Rolls back an accepted proposal and records the reason in its history.
    pub fn rollback(
        &mut self,
        id: &CruxId,
        reason: Option<String>,
    ) -> Result<ProposalLifecycle, ProposalServiceError> {
        let mut proposal = self.load(id)?;
        proposal.rollback(reason)?;
        self.save(proposal)
    }

    fn load(&self, id: &CruxId) -> Result<ProposalLifecycle, ProposalServiceError> {
        self.store
            .get(id)?
            .ok_or_else(|| ProposalServiceError::NotFound(id.to_string()))
    }

    fn save(
        &mut self,
        proposal: ProposalLifecycle,
    ) -> Result<ProposalLifecycle, ProposalServiceError> {
        self.store.update(proposal.clone())?;
        Ok(proposal)
    }

    fn require_passing_evaluation(
        proposal: &ProposalLifecycle,
    ) -> Result<(), ProposalServiceError> {
        match proposal.regression_evaluation.as_ref() {
            None => Err(ProposalServiceError::EvaluationMissing),
            Some(evaluation) if !evaluation.passed => Err(ProposalServiceError::EvaluationFailed(
                evaluation.failures.clone(),
            )),
            Some(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::{
        CandidateTracePolicy, Crux, DefaultStrategyPolicy, EvalThresholds, ImprovementKind,
        PromptPatch, StrategyDiff, StructuralPolicy, TraceDiffMode,
        proposal::{HistoryEvent, ProposalStatus},
        store::InMemoryProposalStore,
    };

    fn improvement(diff: StrategyDiff) -> Improvement {
        Improvement {
            id: CruxId::new(),
            kind: ImprovementKind::ToolPreference,
            target: "rg".into(),
            diff,
            confidence: 0.9,
            evidence: vec![],
            proposed_at: Utc::now(),
        }
    }

    fn tool_diff(value: i32) -> StrategyDiff {
        StrategyDiff {
            tool_preferences: vec![("rg".into(), value)],
            ..Default::default()
        }
    }

    fn prompt_diff() -> StrategyDiff {
        StrategyDiff {
            prompt_patches: vec![PromptPatch {
                agent: "planner".into(),
                section: "system".into(),
                content: "be brief".into(),
            }],
            ..Default::default()
        }
    }

    fn strategy_policy() -> DefaultStrategyPolicy {
        DefaultStrategyPolicy {
            max_tool_pref: Some(10),
        }
    }

    fn regression_policy(expected: Option<serde_json::Value>) -> RegressionPolicy {
        RegressionPolicy {
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
            expected_output: expected,
            invariants: vec![],
        }
    }

    fn run(value: serde_json::Value) -> PersistedRun {
        PersistedRun {
            id: CruxId::new(),
            agent: "test".into(),
            recorded_at: Utc::now(),
            trace: Crux {
                id: CruxId::new(),
                agent: "test".into(),
                pipeline_version: None,
                value: Ok(value),
                steps: vec![],
                children: vec![],
                started_at: Utc::now(),
                finished_at: Some(Utc::now()),
            },
        }
    }

    fn service() -> ProposalService<InMemoryProposalStore> {
        ProposalService::new(InMemoryProposalStore::new())
    }

    /// Attaches runs whose outputs differ from the golden answer `42`, so an
    /// evaluation with `expected_output: Some(42)` fails.
    fn attach_failing_runs(svc: &mut ProposalService<InMemoryProposalStore>, id: &CruxId) {
        svc.attach_runs(id, run(json!({"answer": 41})), run(json!({"answer": 41})))
            .unwrap();
    }

    fn attach_passing_runs(svc: &mut ProposalService<InMemoryProposalStore>, id: &CruxId) {
        svc.attach_runs(id, run(json!({"answer": 42})), run(json!({"answer": 42})))
            .unwrap();
    }

    #[test]
    fn propose_persists_draft_proposal() {
        let mut svc = service();
        let p = svc.propose(improvement(tool_diff(5))).unwrap();
        assert_eq!(p.status, ProposalStatus::Draft);
        let stored = svc.store().get(&p.id).unwrap().expect("persisted");
        assert_eq!(stored.status, ProposalStatus::Draft);
    }

    #[test]
    fn missing_proposal_is_not_found() {
        let svc = service();
        let err = svc.get(&CruxId::new()).unwrap_err();
        assert!(matches!(err, ProposalServiceError::NotFound(_)));
    }

    #[test]
    fn validate_violation_marks_validation_failed() {
        let mut svc = service();
        let p = svc.propose(improvement(tool_diff(99))).unwrap();
        let p = svc.validate(&p.id, &strategy_policy()).unwrap();
        assert!(matches!(p.status, ProposalStatus::ValidationFailed { .. }));
        assert!(!p.policy_validation.as_ref().unwrap().passed);
    }

    #[test]
    fn validate_pass_without_patches_moves_to_validation_passed() {
        let mut svc = service();
        let p = svc.propose(improvement(tool_diff(5))).unwrap();
        let p = svc.validate(&p.id, &strategy_policy()).unwrap();
        assert_eq!(p.status, ProposalStatus::ValidationPassed);
    }

    #[test]
    fn validate_requests_approval_when_policy_requires_it() {
        let mut svc = service();
        let p = svc.propose(improvement(prompt_diff())).unwrap();
        let p = svc.validate(&p.id, &strategy_policy()).unwrap();
        assert_eq!(p.status, ProposalStatus::AwaitingApproval);
        assert!(p.approval.as_ref().unwrap().required);
    }

    #[test]
    fn promote_requires_stored_evaluation() {
        let mut svc = service();
        let p = svc.propose(improvement(tool_diff(5))).unwrap();
        svc.validate(&p.id, &strategy_policy()).unwrap();
        let err = svc.promote(&p.id).unwrap_err();
        assert!(matches!(err, ProposalServiceError::EvaluationMissing));
    }

    #[test]
    fn promote_refuses_failing_evaluation() {
        let mut svc = service();
        let p = svc.propose(improvement(tool_diff(5))).unwrap();
        svc.validate(&p.id, &strategy_policy()).unwrap();
        attach_failing_runs(&mut svc, &p.id);
        let eval = svc
            .evaluate(&p.id, &regression_policy(Some(json!({"answer": 42}))))
            .unwrap();
        assert!(!eval.passed);

        let err = svc.promote(&p.id).unwrap_err();
        assert!(matches!(err, ProposalServiceError::EvaluationFailed(_)));
        let stored = svc.get(&p.id).unwrap();
        assert_eq!(stored.status, ProposalStatus::ValidationPassed);
    }

    #[test]
    fn promote_accepts_after_passing_evaluation_and_persists() {
        let mut svc = service();
        let p = svc.propose(improvement(tool_diff(5))).unwrap();
        svc.validate(&p.id, &strategy_policy()).unwrap();
        attach_passing_runs(&mut svc, &p.id);
        svc.evaluate(&p.id, &regression_policy(Some(json!({"answer": 42}))))
            .unwrap();

        let p = svc.promote(&p.id).unwrap();
        assert!(matches!(p.status, ProposalStatus::Accepted { .. }));
        let stored = svc.get(&p.id).unwrap();
        assert!(matches!(stored.status, ProposalStatus::Accepted { .. }));
        assert!(stored.regression_evaluation.is_some());
    }

    #[test]
    fn evaluation_is_allowed_while_awaiting_approval() {
        let mut svc = service();
        let p = svc.propose(improvement(prompt_diff())).unwrap();
        svc.validate(&p.id, &strategy_policy()).unwrap();
        attach_passing_runs(&mut svc, &p.id);
        let eval = svc
            .evaluate(&p.id, &regression_policy(Some(json!({"answer": 42}))))
            .expect("gatekeeper needs replay evidence before deciding");
        assert!(eval.passed);
    }

    #[test]
    fn granting_approval_refused_without_passing_evaluation() {
        let mut svc = service();
        let p = svc.propose(improvement(prompt_diff())).unwrap();
        svc.validate(&p.id, &strategy_policy()).unwrap();
        attach_failing_runs(&mut svc, &p.id);
        svc.evaluate(&p.id, &regression_policy(Some(json!({"answer": 42}))))
            .unwrap();

        let err = svc
            .decide(&p.id, ApprovalDecision::Granted, "alice", None)
            .unwrap_err();
        assert!(matches!(err, ProposalServiceError::EvaluationFailed(_)));
        assert_eq!(
            svc.get(&p.id).unwrap().status,
            ProposalStatus::AwaitingApproval
        );

        let p = svc
            .decide(
                &p.id,
                ApprovalDecision::Rejected,
                "alice",
                Some("regressed".into()),
            )
            .expect("rejection does not require evaluation");
        assert!(matches!(p.status, ProposalStatus::ApprovalRejected { .. }));
    }

    #[test]
    fn granting_approval_accepts_after_passing_evaluation() {
        let mut svc = service();
        let p = svc.propose(improvement(prompt_diff())).unwrap();
        svc.validate(&p.id, &strategy_policy()).unwrap();
        attach_passing_runs(&mut svc, &p.id);
        svc.evaluate(&p.id, &regression_policy(Some(json!({"answer": 42}))))
            .unwrap();

        let p = svc
            .decide(&p.id, ApprovalDecision::Granted, "alice", None)
            .unwrap();
        assert!(matches!(p.status, ProposalStatus::Accepted { .. }));
        assert_eq!(p.approval.unwrap().decided_by.as_deref(), Some("alice"));
    }

    #[test]
    fn rollback_records_reason_in_history() {
        let mut svc = service();
        let p = svc.propose(improvement(tool_diff(5))).unwrap();
        svc.validate(&p.id, &strategy_policy()).unwrap();
        attach_passing_runs(&mut svc, &p.id);
        svc.evaluate(&p.id, &regression_policy(None)).unwrap();
        svc.promote(&p.id).unwrap();

        let p = svc.rollback(&p.id, Some("latency spike".into())).unwrap();
        assert!(matches!(p.status, ProposalStatus::RolledBack { .. }));
        assert!(matches!(
            p.history.last(),
            Some(HistoryEvent::RolledBack { reason: Some(r), .. }) if r == "latency spike"
        ));
        let stored = svc.get(&p.id).unwrap();
        assert!(matches!(stored.status, ProposalStatus::RolledBack { .. }));
    }
}
