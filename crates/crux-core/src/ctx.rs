/// CruxCtx — the runtime context injected as `t` inside `#[crux::agent]` functions.
///
/// Records steps, manages hooks, tracks budgets, and builds the final `Crux<T>`.
use chrono::Utc;

use crate::types::budget::{Budget, BudgetTracker};
use crate::types::crux_value::Crux;
use crate::types::error::CruxErr;
use crate::types::id::CruxId;
use crate::types::step::{Step, StepKind, StepStatus};

#[derive(Debug)]
pub struct CruxCtx {
    id: CruxId,
    agent_name: String,
    steps: Vec<Step>,
    children: Vec<Crux<serde_json::Value>>,
    budget_tracker: BudgetTracker,
    started_at: chrono::DateTime<Utc>,
    step_ordinal: u32,
}

impl CruxCtx {
    pub fn new(agent_name: &str) -> Self {
        Self {
            id: CruxId::new(),
            agent_name: agent_name.to_string(),
            steps: Vec::new(),
            children: Vec::new(),
            budget_tracker: BudgetTracker::new(Budget::default()),
            started_at: Utc::now(),
            step_ordinal: 0,
        }
    }

    /// Execute a named step, recording it in the trace.
    pub async fn step<F, Fut, T>(
        &mut self,
        name: &str,
        f: F,
    ) -> Result<T, CruxErr>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, CruxErr>>,
        T: serde::Serialize,
    {
        self.step_with_confidence(name, 1.0, f).await
    }

    /// Execute a named step with an explicit confidence score.
    pub async fn step_with_confidence<F, Fut, T>(
        &mut self,
        name: &str,
        confidence: f32,
        f: F,
    ) -> Result<T, CruxErr>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, CruxErr>>,
        T: serde::Serialize,
    {
        let step_start = Utc::now();
        let ordinal = self.step_ordinal;
        self.step_ordinal += 1;

        let input_hash = hash_step_identity(name, ordinal);

        let result = f().await;

        let duration_ms = (Utc::now() - step_start)
            .num_milliseconds()
            .unsigned_abs();

        let (status, output, error) = match &result {
            Ok(val) => (
                StepStatus::Ok,
                serde_json::to_value(val).ok(),
                None,
            ),
            Err(e) => (StepStatus::Err, None, Some(e.to_string())),
        };

        self.steps.push(Step {
            name: name.to_string(),
            kind: StepKind::Plain,
            status,
            confidence,
            started_at: step_start,
            duration_ms,
            input_hash,
            output,
            error,
            attempt: 1,
        });

        result
    }

    /// Get the current budget.
    pub fn budget(&self) -> &Budget {
        self.budget_tracker.budget()
    }

    /// Get remaining budget units.
    pub fn remaining_budget(&self) -> u64 {
        self.budget_tracker.remaining()
    }

    /// Current step ordinal (useful for retry tracking).
    pub fn step_count(&self) -> u32 {
        self.step_ordinal
    }

    /// Snapshot the steps recorded so far (for checkpointing).
    pub fn snapshot_steps(&self) -> &[Step] {
        &self.steps
    }

    /// Finalize the context into a `Crux<T>`.
    pub fn finalize<T>(self, result: Result<T, CruxErr>) -> Crux<T> {
        Crux {
            id: self.id,
            agent: self.agent_name,
            value: result,
            steps: self.steps,
            children: self.children,
            started_at: self.started_at,
            finished_at: Some(Utc::now()),
        }
    }
}

fn hash_step_identity(name: &str, ordinal: u32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    ordinal.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn step_records_success() {
        let mut ctx = CruxCtx::new("test_agent");
        let val = ctx.step("greet", || async { Ok(42) }).await.unwrap();
        assert_eq!(val, 42);
        assert_eq!(ctx.steps.len(), 1);
        assert_eq!(ctx.steps[0].name, "greet");
        assert_eq!(ctx.steps[0].status, StepStatus::Ok);
    }

    #[tokio::test]
    async fn step_records_failure() {
        let mut ctx = CruxCtx::new("test_agent");
        let result: Result<i32, _> = ctx
            .step("fail", || async {
                Err(CruxErr::step_failed("fail", "boom"))
            })
            .await;
        assert!(result.is_err());
        assert_eq!(ctx.steps[0].status, StepStatus::Err);
        assert_eq!(ctx.steps[0].error.as_deref(), Some("step 'fail' failed: boom"));
    }

    #[tokio::test]
    async fn finalize_produces_crux() {
        let mut ctx = CruxCtx::new("hello");
        let _ = ctx.step("a", || async { Ok("hi") }).await;
        let crux = ctx.finalize(Ok("done"));
        assert_eq!(crux.agent, "hello");
        assert_eq!(crux.value().unwrap(), &"done");
        assert_eq!(crux.steps.len(), 1);
        assert!(crux.finished_at.is_some());
    }

    #[tokio::test]
    async fn step_ordinals_increment() {
        let mut ctx = CruxCtx::new("test");
        let _ = ctx.step("a", || async { Ok(1) }).await;
        let _ = ctx.step("b", || async { Ok(2) }).await;
        assert_eq!(ctx.step_count(), 2);
        assert_ne!(ctx.steps[0].input_hash, ctx.steps[1].input_hash);
    }
}
