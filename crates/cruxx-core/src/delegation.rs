/// DelegationBuilder — fluent builder for delegating to a sub-agent.
///
/// Created by `CruxCtx::delegate::<A>(name, input)`. Supports per-call-site
/// budget and lifecycle hooks. The child runs in its own CruxCtx; its Crux
/// is appended to the parent's children.
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

use crate::agent::Agent;
use crate::ctx::CruxCtx;
use crate::types::error::CruxErr;
use crate::types::recovery::Recovery;

type BoxRecoveryFut = Pin<Box<dyn Future<Output = Recovery<serde_json::Value>> + Send>>;

pub struct DelegationBuilder<'a, A: Agent> {
    ctx: &'a mut CruxCtx,
    name: String,
    input: A::Input,
    budget: Option<crate::types::budget::Budget>,
    confidence_threshold: Option<f32>,
    confidence_handler: Option<Box<dyn Fn(f32) -> BoxRecoveryFut + Send + Sync>>,
    failure_handler: Option<Box<dyn Fn(CruxErr) -> BoxRecoveryFut + Send + Sync>>,
    _marker: PhantomData<A>,
}

impl<'a, A: Agent> DelegationBuilder<'a, A>
where
    A::Input: Send,
    A::Output: Send + serde::Serialize + serde::de::DeserializeOwned,
{
    pub(crate) fn new(ctx: &'a mut CruxCtx, name: &str, input: A::Input) -> Self {
        Self {
            ctx,
            name: name.to_string(),
            input,
            budget: None,
            confidence_threshold: None,
            confidence_handler: None,
            failure_handler: None,
            _marker: PhantomData,
        }
    }

    /// Set a budget for the child agent.
    pub fn with_budget(mut self, budget: crate::types::budget::Budget) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Attach a per-call-site low-confidence handler.
    pub fn on_low_confidence<F, Fut>(mut self, threshold: f32, handler: F) -> Self
    where
        F: Fn(f32) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
    {
        self.confidence_threshold = Some(threshold);
        self.confidence_handler = Some(Box::new(move |score| Box::pin(handler(score))));
        self
    }

    /// Attach a per-call-site step-failure handler.
    pub fn on_step_failure<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(CruxErr) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
    {
        self.failure_handler = Some(Box::new(move |err| Box::pin(handler(err))));
        self
    }

    /// Execute the delegation.
    pub async fn run(self) -> Result<A::Output, CruxErr> {
        trace_delegate!(&self.name, A::name());
        let input_hash = self.ctx.next_delegation_hash(&self.name);

        // Create child context
        let mut child_ctx = CruxCtx::new(A::name());
        if let Some(budget) = self.budget {
            child_ctx.set_budget_direct(budget);
        }

        // Wire call-site hooks into the child
        if let (Some(threshold), Some(handler)) =
            (self.confidence_threshold, self.confidence_handler)
        {
            child_ctx
                .hooks_mut()
                .on_low_confidence_boxed(threshold, handler);
        }
        if let Some(handler) = self.failure_handler {
            child_ctx.hooks_mut().on_step_failure_boxed(handler);
        }

        // Run the child agent
        let result = A::run(&mut child_ctx, self.input).await;

        // Finalize child and record in parent
        let child_cruxx = child_ctx.finalize(result);
        let output_val = match &child_cruxx.value {
            Ok(v) => serde_json::to_value(v).ok(),
            Err(_) => None,
        };
        let error_msg = match &child_cruxx.value {
            Ok(_) => None,
            Err(e) => Some(e.to_string()),
        };

        self.ctx.record_delegation_step(
            &self.name,
            input_hash,
            &child_cruxx,
            output_val,
            error_msg,
        );

        match child_cruxx.value {
            Ok(v) => Ok(v),
            Err(e) => Err(CruxErr::Delegation {
                to: A::name().to_string(),
                source: Box::new(e),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Context as _;
    use crate::types::budget::Budget;
    use crate::types::error::CruxErr;

    // Minimal agent that doubles its input.
    struct DoubleAgent;

    impl crate::agent::Agent for DoubleAgent {
        type Input = i32;
        type Output = i32;

        fn name() -> &'static str {
            "double"
        }

        async fn run(
            ctx: &mut crate::ctx::CruxCtx,
            input: Self::Input,
        ) -> Result<Self::Output, CruxErr> {
            ctx.step("double_step", || async move { Ok(input * 2) })
                .await
        }
    }

    // Agent that always fails.
    struct FailAgent;

    impl crate::agent::Agent for FailAgent {
        type Input = ();
        type Output = i32;

        fn name() -> &'static str {
            "fail"
        }

        async fn run(
            _ctx: &mut crate::ctx::CruxCtx,
            _input: Self::Input,
        ) -> Result<Self::Output, CruxErr> {
            Err(CruxErr::step_failed("fail", "always fails"))
        }
    }

    #[tokio::test]
    async fn builder_runs_child_agent_and_returns_output() {
        let mut ctx = CruxCtx::new("parent");
        let result = DelegationBuilder::<DoubleAgent>::new(&mut ctx, "double_it", 21)
            .run()
            .await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn builder_records_delegation_step_in_parent() {
        let mut ctx = CruxCtx::new("parent");
        DelegationBuilder::<DoubleAgent>::new(&mut ctx, "my_step", 5)
            .run()
            .await
            .unwrap();

        let cruxx = ctx.finalize::<i32>(Ok(0));
        assert_eq!(cruxx.steps.len(), 1);
        assert_eq!(cruxx.steps[0].name, "my_step");
        assert_eq!(
            cruxx.steps[0].kind,
            crate::types::step::StepKind::Delegation
        );
    }

    #[tokio::test]
    async fn builder_with_budget_propagates_to_child() {
        let mut ctx = CruxCtx::new("parent");
        let result = DelegationBuilder::<DoubleAgent>::new(&mut ctx, "budgeted", 7)
            .with_budget(Budget::calls(100))
            .run()
            .await;
        assert_eq!(result.unwrap(), 14);
    }

    #[tokio::test]
    async fn failing_agent_returns_delegation_error() {
        let mut ctx = CruxCtx::new("parent");
        let err = DelegationBuilder::<FailAgent>::new(&mut ctx, "will_fail", ())
            .run()
            .await
            .unwrap_err();

        assert!(
            matches!(err, CruxErr::Delegation { ref to, .. } if to == "fail"),
            "expected Delegation error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn failing_agent_records_err_step() {
        let mut ctx = CruxCtx::new("parent");
        let _ = DelegationBuilder::<FailAgent>::new(&mut ctx, "fail_step", ())
            .run()
            .await;

        let cruxx = ctx.finalize::<()>(Ok(()));
        assert_eq!(cruxx.steps.len(), 1);
        assert_eq!(cruxx.steps[0].status, crate::types::step::StepStatus::Err);
    }

    #[tokio::test]
    async fn builder_appends_child_cruxx_to_parent() {
        let mut ctx = CruxCtx::new("parent");
        DelegationBuilder::<DoubleAgent>::new(&mut ctx, "child_run", 3)
            .run()
            .await
            .unwrap();

        let cruxx = ctx.finalize::<i32>(Ok(0));
        assert_eq!(cruxx.children.len(), 1);
        assert_eq!(cruxx.children[0].agent, "double");
    }
}
