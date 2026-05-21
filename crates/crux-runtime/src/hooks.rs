/// HookRegistry — stores and invokes scoped lifecycle hooks.
///
/// Single responsibility: hook storage and dispatch. No step recording,
/// no budget logic, no replay. CruxCtx delegates hook operations here.
use std::future::Future;
use std::pin::Pin;

use crate::types::budget::Budget;
use crate::types::error::CruxErr;
use crate::types::recovery::Recovery;

/// Pre-step gate verdict — determines whether a step is allowed to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookVerdict {
    /// Allow the step to proceed.
    Allow,
    /// Deny the step with a reason. The step will be recorded as skipped.
    Deny(String),
}

/// Boxed sync gate that inspects a step name before execution.
type PreStepGate = Box<dyn Fn(&str) -> HookVerdict + Send + Sync>;

/// Boxed async handler for low-confidence recovery.
type ConfidenceHandler = Box<
    dyn Fn(f32) -> Pin<Box<dyn Future<Output = Recovery<serde_json::Value>> + Send>> + Send + Sync,
>;

/// Boxed async handler for step-failure recovery.
type FailureHandler = Box<
    dyn Fn(CruxErr) -> Pin<Box<dyn Future<Output = Recovery<serde_json::Value>> + Send>>
        + Send
        + Sync,
>;

/// Boxed async handler for budget-exceeded recovery.
type BudgetHandler = Box<
    dyn Fn(Budget) -> Pin<Box<dyn Future<Output = Recovery<serde_json::Value>> + Send>>
        + Send
        + Sync,
>;

/// Boxed async handler for approval-required events.
type ApprovalHandler = Box<
    dyn Fn(serde_json::Value) -> Pin<Box<dyn Future<Output = Recovery<serde_json::Value>> + Send>>
        + Send
        + Sync,
>;

pub struct HookRegistry {
    pub(crate) confidence_threshold: Option<f32>,
    confidence_handler: Option<ConfidenceHandler>,
    failure_handler: Option<FailureHandler>,
    budget_handler: Option<BudgetHandler>,
    approval_handler: Option<ApprovalHandler>,
    pre_step_gate: Option<PreStepGate>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            confidence_threshold: None,
            confidence_handler: None,
            failure_handler: None,
            budget_handler: None,
            approval_handler: None,
            pre_step_gate: None,
        }
    }

    /// Register a pre-step gate. If the gate returns `Deny`, the step is
    /// skipped and recorded with status `Skipped`.
    pub fn on_pre_step<F>(&mut self, gate: F)
    where
        F: Fn(&str) -> HookVerdict + Send + Sync + 'static,
    {
        self.pre_step_gate = Some(Box::new(gate));
    }

    /// Check the pre-step gate for a given step name. Returns `Allow` if no
    /// gate is registered.
    pub fn check_pre_step(&self, step_name: &str) -> HookVerdict {
        match &self.pre_step_gate {
            Some(gate) => gate(step_name),
            None => HookVerdict::Allow,
        }
    }

    /// Register a low-confidence handler. Fires when step confidence < threshold.
    pub fn on_low_confidence<F, Fut>(&mut self, threshold: f32, handler: F)
    where
        F: Fn(f32) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
    {
        self.confidence_threshold = Some(threshold);
        self.confidence_handler = Some(Box::new(move |score| Box::pin(handler(score))));
    }

    /// Register a step-failure handler.
    pub fn on_step_failure<F, Fut>(&mut self, handler: F)
    where
        F: Fn(CruxErr) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
    {
        self.failure_handler = Some(Box::new(move |err| Box::pin(handler(err))));
    }

    /// Register a budget-exceeded handler.
    pub fn on_budget_exceeded<F, Fut>(&mut self, handler: F)
    where
        F: Fn(Budget) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
    {
        self.budget_handler = Some(Box::new(move |budget| Box::pin(handler(budget))));
    }

    /// Invoke the low-confidence handler if registered and confidence < threshold.
    /// Returns None if no handler or confidence is above threshold.
    pub async fn check_confidence(&self, confidence: f32) -> Option<Recovery<serde_json::Value>> {
        if let (Some(threshold), Some(handler)) =
            (self.confidence_threshold, &self.confidence_handler)
            && confidence < threshold
        {
            return Some(handler(confidence).await);
        }
        None
    }

    /// Invoke the step-failure handler if registered.
    pub async fn check_failure(&self, err: CruxErr) -> Option<Recovery<serde_json::Value>> {
        if let Some(handler) = &self.failure_handler {
            Some(handler(err).await)
        } else {
            None
        }
    }

    /// Invoke the budget-exceeded handler if registered.
    pub async fn check_budget(&self, budget: Budget) -> Option<Recovery<serde_json::Value>> {
        if let Some(handler) = &self.budget_handler {
            Some(handler(budget).await)
        } else {
            None
        }
    }

    /// Register a pre-boxed confidence handler (used by DelegationBuilder).
    pub(crate) fn on_low_confidence_boxed(&mut self, threshold: f32, handler: ConfidenceHandler) {
        self.confidence_threshold = Some(threshold);
        self.confidence_handler = Some(handler);
    }

    /// Register a pre-boxed failure handler (used by DelegationBuilder).
    pub(crate) fn on_step_failure_boxed(&mut self, handler: FailureHandler) {
        self.failure_handler = Some(handler);
    }

    pub fn has_failure_handler(&self) -> bool {
        self.failure_handler.is_some()
    }

    /// Register an approval-required handler. Fires when a step needs gate approval.
    pub fn on_approval_required<F, Fut>(&mut self, handler: F)
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
    {
        self.approval_handler = Some(Box::new(move |req| Box::pin(handler(req))));
    }

    /// Invoke the approval handler if registered.
    pub async fn check_approval(
        &self,
        request: serde_json::Value,
    ) -> Option<Recovery<serde_json::Value>> {
        if let Some(handler) = &self.approval_handler {
            Some(handler(request).await)
        } else {
            None
        }
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for HookRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookRegistry")
            .field("confidence_threshold", &self.confidence_threshold)
            .field("has_confidence_handler", &self.confidence_handler.is_some())
            .field("has_failure_handler", &self.failure_handler.is_some())
            .field("has_budget_handler", &self.budget_handler.is_some())
            .field("has_approval_handler", &self.approval_handler.is_some())
            .field("has_pre_step_gate", &self.pre_step_gate.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn confidence_fires_below_threshold() {
        let mut hooks = HookRegistry::new();
        hooks.on_low_confidence(0.8, |_| async { Recovery::Continue });
        assert!(hooks.check_confidence(0.5).await.is_some());
    }

    #[tokio::test]
    async fn confidence_skips_above_threshold() {
        let mut hooks = HookRegistry::new();
        hooks.on_low_confidence(0.8, |_| async { Recovery::Continue });
        assert!(hooks.check_confidence(0.9).await.is_none());
    }

    #[tokio::test]
    async fn failure_returns_none_without_handler() {
        let hooks = HookRegistry::new();
        let err = CruxErr::step_failed("x", "y");
        assert!(hooks.check_failure(err).await.is_none());
    }

    #[tokio::test]
    async fn failure_invokes_handler() {
        let mut hooks = HookRegistry::new();
        hooks.on_step_failure(|_| async { Recovery::Propagate });
        let err = CruxErr::step_failed("x", "y");
        assert!(hooks.check_failure(err).await.is_some());
    }

    #[tokio::test]
    async fn budget_returns_none_without_handler() {
        let hooks = HookRegistry::new();
        assert!(hooks.check_budget(Budget::tokens(10)).await.is_none());
    }

    #[tokio::test]
    async fn approval_fires_when_registered() {
        let mut hooks = HookRegistry::new();
        hooks.on_approval_required(|req| async move {
            let _ = req;
            Recovery::Continue
        });
        let request = serde_json::json!({"summary": "enable network"});
        let result = hooks.check_approval(request).await;
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn approval_returns_none_without_handler() {
        let hooks = HookRegistry::new();
        let request = serde_json::json!({"summary": "enable network"});
        assert!(hooks.check_approval(request).await.is_none());
    }

    #[test]
    fn pre_step_gate_allows_by_default() {
        let hooks = HookRegistry::new();
        assert_eq!(hooks.check_pre_step("anything"), HookVerdict::Allow);
    }

    #[test]
    fn pre_step_gate_denies_blocked_steps() {
        let mut hooks = HookRegistry::new();
        hooks.on_pre_step(|name| {
            if name.starts_with("dangerous_") {
                HookVerdict::Deny("blocked by policy".into())
            } else {
                HookVerdict::Allow
            }
        });
        assert_eq!(hooks.check_pre_step("safe_step"), HookVerdict::Allow);
        assert_eq!(
            hooks.check_pre_step("dangerous_rm"),
            HookVerdict::Deny("blocked by policy".into())
        );
    }
}
