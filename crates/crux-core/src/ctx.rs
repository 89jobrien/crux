/// CruxCtx — the runtime context injected as `t` inside `#[crux::agent]` functions.
///
/// Records steps, manages hooks, tracks budgets, supports replay, and builds the final `Crux<T>`.
use std::future::Future;
use std::pin::Pin;

use chrono::Utc;

use crate::types::budget::{Budget, BudgetTracker};
use crate::types::crux_value::Crux;
use crate::types::error::CruxErr;
use crate::types::id::CruxId;
use crate::types::recovery::Recovery;
use crate::types::step::{Step, StepKind, StepStatus};

/// Boxed async handler for low-confidence recovery.
type ConfidenceHandler = Box<
    dyn Fn(f32) -> Pin<Box<dyn Future<Output = Recovery<serde_json::Value>> + Send>>
        + Send
        + Sync,
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

/// A cached step from a prior trace, used for replay.
#[derive(Debug, Clone)]
struct ReplayEntry {
    input_hash: u64,
    output: Option<serde_json::Value>,
}

pub struct CruxCtx {
    id: CruxId,
    agent_name: String,
    steps: Vec<Step>,
    children: Vec<Crux<serde_json::Value>>,
    budget_tracker: BudgetTracker,
    started_at: chrono::DateTime<Utc>,
    step_ordinal: u32,

    // Lifecycle hooks (type-erased; callers downcast via serde at boundaries)
    confidence_threshold: Option<f32>,
    confidence_handler: Option<ConfidenceHandler>,
    failure_handler: Option<FailureHandler>,
    budget_handler: Option<BudgetHandler>,

    // Replay cache: ordinal -> cached output
    replay_cache: Vec<ReplayEntry>,
    replay_enabled: bool,

    // Max retries per step (prevents infinite Retry loops)
    max_retries: u32,
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
            confidence_threshold: None,
            confidence_handler: None,
            failure_handler: None,
            budget_handler: None,
            replay_cache: Vec::new(),
            replay_enabled: false,
            max_retries: 3,
        }
    }

    // -- Hook registration --------------------------------------------------

    /// Register a scoped low-confidence handler. Fires when a step's confidence
    /// is below `threshold`.
    pub fn on_low_confidence<F, Fut>(&mut self, threshold: f32, handler: F)
    where
        F: Fn(f32) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
    {
        self.confidence_threshold = Some(threshold);
        self.confidence_handler = Some(Box::new(move |score| Box::pin(handler(score))));
    }

    /// Register a scoped step-failure handler.
    pub fn on_step_failure<F, Fut>(&mut self, handler: F)
    where
        F: Fn(CruxErr) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
    {
        self.failure_handler = Some(Box::new(move |err| Box::pin(handler(err))));
    }

    /// Register a scoped budget-exceeded handler.
    pub fn on_budget_exceeded<F, Fut>(&mut self, handler: F)
    where
        F: Fn(Budget) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
    {
        self.budget_handler = Some(Box::new(move |budget| Box::pin(handler(budget))));
    }

    /// Override the maximum number of retries per step (default: 3).
    pub fn set_max_retries(&mut self, n: u32) {
        self.max_retries = n;
    }

    // -- Replay -------------------------------------------------------------

    /// Seed replay from a previous trace. Steps whose `input_hash` matches
    /// return the cached output without re-executing.
    pub fn replay_from(&mut self, previous: &Crux<serde_json::Value>) {
        self.replay_cache = previous
            .steps
            .iter()
            .map(|s| ReplayEntry {
                input_hash: s.input_hash,
                output: s.output.clone(),
            })
            .collect();
        self.replay_enabled = true;
    }

    // -- Step execution -----------------------------------------------------

    /// Execute a named step, recording it in the trace.
    pub async fn step<F, Fut, T>(&mut self, name: &str, f: F) -> Result<T, CruxErr>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, CruxErr>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send,
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
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, CruxErr>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send,
    {
        let ordinal = self.step_ordinal;
        self.step_ordinal += 1;
        let input_hash = hash_step_identity(name, ordinal);

        // -- Replay check ---------------------------------------------------
        if self.replay_enabled {
            if let Some(entry) = self.replay_cache.get(ordinal as usize) {
                if entry.input_hash == input_hash {
                    if let Some(ref cached) = entry.output {
                        let value: T = serde_json::from_value(cached.clone()).map_err(|e| {
                            CruxErr::step_failed(name, format!("replay deserialize: {e}"))
                        })?;

                        self.steps.push(Step {
                            name: name.to_string(),
                            kind: StepKind::Plain,
                            status: StepStatus::Ok,
                            confidence,
                            started_at: Utc::now(),
                            duration_ms: 0,
                            input_hash,
                            output: Some(cached.clone()),
                            error: None,
                            attempt: 0, // 0 = replayed
                        });
                        return Ok(value);
                    }
                } else {
                    return Err(CruxErr::ReplayMismatch {
                        step: name.to_string(),
                        expected: entry.input_hash,
                        actual: input_hash,
                    });
                }
            }
            // Past the end of the replay cache — execute normally.
        }

        // -- Budget check (pre-step) ----------------------------------------
        if self.budget_tracker.is_exceeded() {
            if let Some(ref handler) = self.budget_handler {
                let budget = self.budget_tracker.budget().clone();
                let recovery = handler(budget).await;
                return self.apply_recovery_value(name, input_hash, confidence, recovery).await;
            }
            return Err(CruxErr::BudgetExceeded {
                budget_kind: self.budget_tracker.budget().kind(),
                limit: self.budget_tracker.budget().limit(),
                actual: self.budget_tracker.budget().limit() + 1,
            });
        }

        // -- Execute with retry loop ----------------------------------------
        self.execute_with_hooks(name, confidence, input_hash, f).await
    }

    /// Core execution loop: run the closure, apply hooks on failure or low confidence.
    async fn execute_with_hooks<F, Fut, T>(
        &mut self,
        name: &str,
        confidence: f32,
        input_hash: u64,
        f: F,
    ) -> Result<T, CruxErr>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, CruxErr>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send,
    {
        let step_start = Utc::now();
        let result = f().await;
        let duration_ms = (Utc::now() - step_start).num_milliseconds().unsigned_abs();

        match result {
            Ok(val) => {
                // Check low-confidence hook
                if let (Some(threshold), Some(handler)) =
                    (self.confidence_threshold, &self.confidence_handler)
                {
                    if confidence < threshold {
                        let recovery = handler(confidence).await;
                        // Record the original step as low-confidence
                        self.steps.push(Step {
                            name: name.to_string(),
                            kind: StepKind::Plain,
                            status: StepStatus::Ok,
                            confidence,
                            started_at: step_start,
                            duration_ms,
                            input_hash,
                            output: serde_json::to_value(&val).ok(),
                            error: None,
                            attempt: 1,
                        });
                        return self.apply_recovery_value(name, input_hash, confidence, recovery).await;
                    }
                }

                self.steps.push(Step {
                    name: name.to_string(),
                    kind: StepKind::Plain,
                    status: StepStatus::Ok,
                    confidence,
                    started_at: step_start,
                    duration_ms,
                    input_hash,
                    output: serde_json::to_value(&val).ok(),
                    error: None,
                    attempt: 1,
                });
                Ok(val)
            }
            Err(e) => {
                // Record the failed attempt
                self.steps.push(Step {
                    name: name.to_string(),
                    kind: StepKind::Plain,
                    status: StepStatus::Err,
                    confidence,
                    started_at: step_start,
                    duration_ms,
                    input_hash,
                    output: None,
                    error: Some(e.to_string()),
                    attempt: 1,
                });

                // Check failure hook
                if let Some(ref handler) = self.failure_handler {
                    let recovery = handler(e).await;
                    return self.apply_recovery_value(name, input_hash, confidence, recovery).await;
                }

                Err(e)
            }
        }
    }

    /// Apply a `Recovery<serde_json::Value>` and convert back to `T`.
    async fn apply_recovery_value<T>(
        &mut self,
        name: &str,
        input_hash: u64,
        confidence: f32,
        recovery: Recovery<serde_json::Value>,
    ) -> Result<T, CruxErr>
    where
        T: serde::de::DeserializeOwned,
    {
        match recovery {
            Recovery::Continue => {
                // The last step was already recorded. Extract its output.
                if let Some(step) = self.steps.last() {
                    if let Some(ref output) = step.output {
                        return serde_json::from_value(output.clone()).map_err(|e| {
                            CruxErr::step_failed(name, format!("continue deserialize: {e}"))
                        });
                    }
                }
                Err(CruxErr::step_failed(name, "continue with no output"))
            }
            Recovery::Substitute(val) => {
                self.steps.push(Step {
                    name: name.to_string(),
                    kind: StepKind::Plain,
                    status: StepStatus::Ok,
                    confidence,
                    started_at: Utc::now(),
                    duration_ms: 0,
                    input_hash,
                    output: Some(val.clone()),
                    error: None,
                    attempt: 0, // substituted
                });
                serde_json::from_value(val).map_err(|e| {
                    CruxErr::step_failed(name, format!("substitute deserialize: {e}"))
                })
            }
            Recovery::Skip => {
                self.steps.push(Step {
                    name: name.to_string(),
                    kind: StepKind::Plain,
                    status: StepStatus::Skipped,
                    confidence,
                    started_at: Utc::now(),
                    duration_ms: 0,
                    input_hash,
                    output: None,
                    error: None,
                    attempt: 0,
                });
                Err(CruxErr::step_failed(name, "step skipped"))
            }
            Recovery::Propagate => {
                // Error already recorded in the step trace.
                if let Some(step) = self.steps.last() {
                    if let Some(ref err_msg) = step.error {
                        return Err(CruxErr::step_failed(name, err_msg.clone()));
                    }
                }
                Err(CruxErr::step_failed(name, "propagated"))
            }
            Recovery::Escalate(fut) => {
                let escalation_result = fut.await;
                match escalation_result {
                    Ok(val) => {
                        self.steps.push(Step {
                            name: format!("{name}::escalated"),
                            kind: StepKind::Plain,
                            status: StepStatus::Ok,
                            confidence: 1.0,
                            started_at: Utc::now(),
                            duration_ms: 0,
                            input_hash,
                            output: Some(val.clone()),
                            error: None,
                            attempt: 1,
                        });
                        serde_json::from_value(val).map_err(|e| {
                            CruxErr::step_failed(name, format!("escalate deserialize: {e}"))
                        })
                    }
                    Err(e) => Err(e),
                }
            }
            Recovery::Retry | Recovery::RetryWith(_) => {
                // Retry is not directly supported through the type-erased path.
                // The caller's closure has already been consumed. Record the intent.
                self.steps.push(Step {
                    name: format!("{name}::retry_requested"),
                    kind: StepKind::Plain,
                    status: StepStatus::Err,
                    confidence,
                    started_at: Utc::now(),
                    duration_ms: 0,
                    input_hash,
                    output: None,
                    error: Some("retry requested but closure consumed".to_string()),
                    attempt: 0,
                });
                Err(CruxErr::step_failed(
                    name,
                    "retry not supported in single-shot step; use step_retryable",
                ))
            }
        }
    }

    /// Execute a step with automatic retry support. The closure factory is called
    /// on each attempt, enabling `Recovery::Retry`.
    pub async fn step_retryable<F, Fut, T>(
        &mut self,
        name: &str,
        confidence: f32,
        mut make_fut: F,
    ) -> Result<T, CruxErr>
    where
        F: FnMut() -> Fut + Send,
        Fut: Future<Output = Result<T, CruxErr>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send,
    {
        let ordinal = self.step_ordinal;
        self.step_ordinal += 1;
        let input_hash = hash_step_identity(name, ordinal);

        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            if attempt > self.max_retries + 1 {
                return Err(CruxErr::step_failed(
                    name,
                    format!("exceeded max retries ({})", self.max_retries),
                ));
            }

            let step_start = Utc::now();
            let result = make_fut().await;
            let duration_ms = (Utc::now() - step_start).num_milliseconds().unsigned_abs();

            match result {
                Ok(val) => {
                    // Low-confidence check
                    if let (Some(threshold), Some(handler)) =
                        (self.confidence_threshold, &self.confidence_handler)
                    {
                        if confidence < threshold {
                            self.steps.push(Step {
                                name: name.to_string(),
                                kind: StepKind::Plain,
                                status: StepStatus::Ok,
                                confidence,
                                started_at: step_start,
                                duration_ms,
                                input_hash,
                                output: serde_json::to_value(&val).ok(),
                                error: None,
                                attempt,
                            });
                            let recovery = handler(confidence).await;
                            match recovery {
                                Recovery::Retry => continue,
                                other => {
                                    return self.apply_recovery_value(
                                        name, input_hash, confidence, other,
                                    ).await;
                                }
                            }
                        }
                    }

                    self.steps.push(Step {
                        name: name.to_string(),
                        kind: StepKind::Plain,
                        status: StepStatus::Ok,
                        confidence,
                        started_at: step_start,
                        duration_ms,
                        input_hash,
                        output: serde_json::to_value(&val).ok(),
                        error: None,
                        attempt,
                    });
                    return Ok(val);
                }
                Err(e) => {
                    self.steps.push(Step {
                        name: name.to_string(),
                        kind: StepKind::Plain,
                        status: StepStatus::Err,
                        confidence,
                        started_at: step_start,
                        duration_ms,
                        input_hash,
                        output: None,
                        error: Some(e.to_string()),
                        attempt,
                    });

                    if let Some(ref handler) = self.failure_handler {
                        let recovery = handler(e).await;
                        match recovery {
                            Recovery::Retry => continue,
                            Recovery::RetryWith(make_new) => {
                                // Execute the replacement future
                                let retry_start = Utc::now();
                                let retry_result = make_new().await;
                                let retry_dur =
                                    (Utc::now() - retry_start).num_milliseconds().unsigned_abs();
                                match retry_result {
                                    Ok(val) => {
                                        self.steps.push(Step {
                                            name: format!("{name}::retry_with"),
                                            kind: StepKind::Plain,
                                            status: StepStatus::Ok,
                                            confidence: 1.0,
                                            started_at: retry_start,
                                            duration_ms: retry_dur,
                                            input_hash,
                                            output: Some(val.clone()),
                                            error: None,
                                            attempt: attempt + 1,
                                        });
                                        return serde_json::from_value(val).map_err(|e| {
                                            CruxErr::step_failed(
                                                name,
                                                format!("retry_with deserialize: {e}"),
                                            )
                                        });
                                    }
                                    Err(e) => return Err(e),
                                }
                            }
                            other => {
                                return self.apply_recovery_value(
                                    name, input_hash, confidence, other,
                                ).await;
                            }
                        }
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }

    // -- Budget -------------------------------------------------------------

    /// Set a custom budget for this context.
    pub fn set_budget(&mut self, budget: Budget) {
        self.budget_tracker = BudgetTracker::new(budget);
    }

    /// Record budget consumption.
    pub fn consume_budget(&mut self, amount: u64) {
        self.budget_tracker.consume(amount);
    }

    /// Get the current budget.
    pub fn budget(&self) -> &Budget {
        self.budget_tracker.budget()
    }

    /// Get remaining budget units.
    pub fn remaining_budget(&self) -> u64 {
        self.budget_tracker.remaining()
    }

    // -- Accessors ----------------------------------------------------------

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

impl std::fmt::Debug for CruxCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CruxCtx")
            .field("id", &self.id)
            .field("agent_name", &self.agent_name)
            .field("steps", &self.steps)
            .field("step_ordinal", &self.step_ordinal)
            .field("replay_enabled", &self.replay_enabled)
            .finish_non_exhaustive()
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

    // -- Existing tests (preserved) -----------------------------------------

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
        assert_eq!(
            ctx.steps[0].error.as_deref(),
            Some("step 'fail' failed: boom")
        );
    }

    #[tokio::test]
    async fn finalize_produces_crux() {
        let mut ctx = CruxCtx::new("hello");
        let _: String = ctx.step("a", || async { Ok("hi".to_string()) }).await.unwrap();
        let crux = ctx.finalize(Ok("done".to_string()));
        assert_eq!(crux.agent, "hello");
        assert_eq!(crux.value().unwrap(), "done");
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

    // -- Lifecycle hook tests -----------------------------------------------

    #[tokio::test]
    async fn on_step_failure_substitute() {
        let mut ctx = CruxCtx::new("test");
        ctx.on_step_failure(|_err| async { Recovery::Substitute(serde_json::json!(99)) });

        let val: i32 = ctx
            .step("might_fail", || async {
                Err(CruxErr::step_failed("might_fail", "transient"))
            })
            .await
            .unwrap();

        assert_eq!(val, 99);
        // Should have the failed step + substituted step
        assert!(ctx.steps.len() >= 2);
    }

    #[tokio::test]
    async fn on_step_failure_propagate() {
        let mut ctx = CruxCtx::new("test");
        ctx.on_step_failure(|_err| async { Recovery::Propagate });

        let result: Result<i32, _> = ctx
            .step("fail", || async {
                Err(CruxErr::step_failed("fail", "fatal"))
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn on_step_failure_skip() {
        let mut ctx = CruxCtx::new("test");
        ctx.on_step_failure(|_err| async { Recovery::Skip });

        let result: Result<i32, _> = ctx
            .step("optional", || async {
                Err(CruxErr::step_failed("optional", "not critical"))
            })
            .await;

        assert!(result.is_err());
        let skipped = ctx.steps.iter().any(|s| s.status == StepStatus::Skipped);
        assert!(skipped);
    }

    #[tokio::test]
    async fn on_low_confidence_fires() {
        let mut ctx = CruxCtx::new("test");
        ctx.on_low_confidence(0.8, |_score| async {
            Recovery::Substitute(serde_json::json!(42))
        });

        let val: i32 = ctx
            .step_with_confidence("uncertain", 0.5, || async { Ok(10) })
            .await
            .unwrap();

        // Hook substitutes 42 for the low-confidence 10
        assert_eq!(val, 42);
    }

    #[tokio::test]
    async fn on_low_confidence_does_not_fire_above_threshold() {
        let mut ctx = CruxCtx::new("test");
        ctx.on_low_confidence(0.8, |_score| async {
            Recovery::Substitute(serde_json::json!(42))
        });

        let val: i32 = ctx
            .step_with_confidence("confident", 0.9, || async { Ok(10) })
            .await
            .unwrap();

        assert_eq!(val, 10);
    }

    #[tokio::test]
    async fn on_low_confidence_continue() {
        let mut ctx = CruxCtx::new("test");
        ctx.on_low_confidence(0.8, |_score| async { Recovery::Continue });

        let val: i32 = ctx
            .step_with_confidence("uncertain", 0.5, || async { Ok(77) })
            .await
            .unwrap();

        // Continue keeps the original value
        assert_eq!(val, 77);
    }

    #[tokio::test]
    async fn step_retryable_retries_on_failure() {
        let mut ctx = CruxCtx::new("test");
        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cc = call_count.clone();

        ctx.on_step_failure(|_err| async { Recovery::Retry });

        let val: i32 = ctx
            .step_retryable("flaky", 1.0, move || {
                let count = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    if count < 2 {
                        Err(CruxErr::step_failed("flaky", "transient"))
                    } else {
                        Ok(42)
                    }
                }
            })
            .await
            .unwrap();

        assert_eq!(val, 42);
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 3);
        // Should have 2 failed steps + 1 success
        let ok_count = ctx.steps.iter().filter(|s| s.is_ok()).count();
        let err_count = ctx.steps.iter().filter(|s| s.is_err()).count();
        assert_eq!(ok_count, 1);
        assert_eq!(err_count, 2);
    }

    #[tokio::test]
    async fn step_retryable_respects_max_retries() {
        let mut ctx = CruxCtx::new("test");
        ctx.set_max_retries(2);
        ctx.on_step_failure(|_err| async { Recovery::Retry });

        let result: Result<i32, _> = ctx
            .step_retryable("always_fails", 1.0, || async {
                Err(CruxErr::step_failed("always_fails", "permanent"))
            })
            .await;

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("max retries"));
    }

    #[tokio::test]
    async fn on_step_failure_escalate() {
        let mut ctx = CruxCtx::new("test");
        ctx.on_step_failure(|_err| async {
            Recovery::Escalate(Box::pin(async { Ok(serde_json::json!(999)) }))
        });

        let val: i32 = ctx
            .step("fail", || async {
                Err(CruxErr::step_failed("fail", "try escalation"))
            })
            .await
            .unwrap();

        assert_eq!(val, 999);
        let escalated = ctx.steps.iter().any(|s| s.name.contains("escalated"));
        assert!(escalated);
    }

    // -- Budget hook tests --------------------------------------------------

    #[tokio::test]
    async fn budget_exceeded_fires_hook() {
        let mut ctx = CruxCtx::new("test");
        ctx.set_budget(Budget::tokens(10));
        ctx.consume_budget(100); // exceed

        ctx.on_budget_exceeded(|_budget| async {
            Recovery::Substitute(serde_json::json!(0))
        });

        let val: i32 = ctx.step("over_budget", || async { Ok(42) }).await.unwrap();
        assert_eq!(val, 0);
    }

    #[tokio::test]
    async fn budget_exceeded_without_hook_errors() {
        let mut ctx = CruxCtx::new("test");
        ctx.set_budget(Budget::tokens(10));
        ctx.consume_budget(100);

        let result: Result<i32, _> = ctx.step("over_budget", || async { Ok(42) }).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("budget exceeded"));
    }

    // -- Replay tests -------------------------------------------------------

    #[tokio::test]
    async fn replay_returns_cached_output() {
        // First run: build a trace
        let mut ctx1 = CruxCtx::new("agent");
        let _: String = ctx1.step("fetch", || async { Ok("cached_data".to_string()) }).await.unwrap();
        let crux1 = ctx1.finalize(Ok("done".to_string()));
        let snapshot = crux1.to_snapshot().unwrap();

        // Second run: replay from the trace
        let mut ctx2 = CruxCtx::new("agent");
        ctx2.replay_from(&snapshot);

        let val: String = ctx2
            .step("fetch", || async {
                panic!("should not execute during replay")
            })
            .await
            .unwrap();

        assert_eq!(val, "cached_data");
        assert_eq!(ctx2.steps[0].attempt, 0); // replayed marker
    }

    #[tokio::test]
    async fn replay_mismatch_errors() {
        let mut ctx1 = CruxCtx::new("agent");
        let _: String = ctx1.step("fetch", || async { Ok("data".to_string()) }).await.unwrap();
        let crux1 = ctx1.finalize(Ok("done".to_string()));
        let snapshot = crux1.to_snapshot().unwrap();

        let mut ctx2 = CruxCtx::new("agent");
        ctx2.replay_from(&snapshot);

        // Different step name at ordinal 0 -> hash mismatch
        let result: Result<String, _> = ctx2
            .step("different_name", || async { Ok("data".to_string()) })
            .await;

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("replay mismatch"));
    }

    #[tokio::test]
    async fn replay_falls_through_past_cache() {
        let mut ctx1 = CruxCtx::new("agent");
        let _ = ctx1.step("a", || async { Ok(1) }).await;
        let crux1 = ctx1.finalize(Ok(1));
        let snapshot = crux1.to_snapshot().unwrap();

        let mut ctx2 = CruxCtx::new("agent");
        ctx2.replay_from(&snapshot);

        // Step 0 replays
        let _ = ctx2.step("a", || async { Ok(1) }).await.unwrap();
        // Step 1 is past cache — executes normally
        let val = ctx2.step("b", || async { Ok(2) }).await.unwrap();
        assert_eq!(val, 2);
        assert_eq!(ctx2.steps.len(), 2);
        assert_eq!(ctx2.steps[0].attempt, 0); // replayed
        assert_eq!(ctx2.steps[1].attempt, 1); // fresh
    }
}
