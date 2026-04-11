/// CruxCtx — the production adapter for the Context trait.
///
/// Coordinates StepRecorder, HookRegistry, BudgetTracker, and ReplayCache.
/// Injected as `t` inside `#[crux::agent]` functions.
use std::future::Future;

use chrono::Utc;

use crate::agent::Agent;
use crate::context::Context;
use crate::hooks::HookRegistry;
use crate::recorder::{StepRecord, StepRecorder};
use crate::replay::{ReplayCache, ReplayResult, deserialize_replay};
use crate::types::budget::{Budget, BudgetTracker};
use crate::types::crux_value::Crux;
use crate::types::error::CruxErr;
use crate::types::id::CruxId;
use crate::types::recovery::Recovery;

#[derive(Debug)]
pub struct CruxCtx {
    id: CruxId,
    agent_name: String,
    recorder: StepRecorder,
    hooks: HookRegistry,
    replay: ReplayCache,
    budget_tracker: BudgetTracker,
    children: Vec<Crux<serde_json::Value>>,
    started_at: chrono::DateTime<Utc>,
    max_retries: u32,
}

impl CruxCtx {
    pub fn new(agent_name: &str) -> Self {
        Self {
            id: CruxId::new(),
            agent_name: agent_name.to_string(),
            recorder: StepRecorder::new(),
            hooks: HookRegistry::new(),
            replay: ReplayCache::new(),
            budget_tracker: BudgetTracker::new(Budget::default()),
            children: Vec::new(),
            started_at: Utc::now(),
            max_retries: 3,
        }
    }

    /// Seed replay from a previous trace.
    pub fn replay_from(&mut self, previous: &Crux<serde_json::Value>) {
        self.replay.seed_from(previous);
    }

    /// Finalize the context into a `Crux<T>`.
    pub fn finalize<T>(self, result: Result<T, CruxErr>) -> Crux<T> {
        Crux {
            id: self.id,
            agent: self.agent_name,
            value: result,
            steps: self.recorder.into_steps(),
            children: self.children,
            started_at: self.started_at,
            finished_at: Some(Utc::now()),
        }
    }

    // -- Internal helpers for delegation/speculation --------------------------

    /// Allocate an ordinal and return the input hash for a delegation step.
    pub(crate) fn next_delegation_hash(&mut self, name: &str) -> u64 {
        let (_ordinal, hash) = self.recorder.next_ordinal(name);
        hash
    }

    /// Set budget directly (used by DelegationBuilder for child contexts).
    pub(crate) fn set_budget_direct(&mut self, budget: Budget) {
        self.budget_tracker = BudgetTracker::new(budget);
    }

    /// Expose hooks for per-call-site hook wiring.
    pub(crate) fn hooks_mut(&mut self) -> &mut HookRegistry {
        &mut self.hooks
    }

    /// Expose recorder for ordinal allocation.
    pub(crate) fn recorder_mut(&mut self) -> &mut StepRecorder {
        &mut self.recorder
    }

    /// Push a raw step (used by delegation/speculation).
    pub(crate) fn push_step(&mut self, step: crate::types::step::Step) {
        self.recorder.push_raw(step);
    }

    /// Push a child crux (used by delegation).
    pub(crate) fn push_child(&mut self, child: Crux<serde_json::Value>) {
        self.children.push(child);
    }

    /// Record a delegation step and append the child crux.
    pub(crate) fn record_delegation_step<T: serde::Serialize>(
        &mut self,
        name: &str,
        input_hash: u64,
        child_crux: &Crux<T>,
        output: Option<serde_json::Value>,
        error: Option<String>,
    ) {
        use crate::types::step::{StepKind, StepStatus};

        let status = if child_crux.value.is_ok() {
            StepStatus::Ok
        } else {
            StepStatus::Err
        };

        self.push_step(crate::types::step::Step {
            name: name.to_string(),
            kind: StepKind::Delegation,
            status,
            confidence: 1.0,
            started_at: child_crux.started_at,
            duration_ms: child_crux.duration_ms().unwrap_or(0),
            input_hash,
            output,
            error,
            attempt: 1,
        });

        if let Ok(snapshot) = child_crux.to_snapshot() {
            self.push_child(snapshot);
        }
    }

    /// Start building a delegation to agent `A`.
    pub fn delegate<'a, A: Agent>(
        &'a mut self,
        name: &str,
        input: A::Input,
    ) -> crate::delegation::DelegationBuilder<'a, A>
    where
        A::Input: Send,
        A::Output: Send + serde::Serialize + serde::de::DeserializeOwned,
    {
        crate::delegation::DelegationBuilder::new(self, name, input)
    }

    /// Start a speculation: run multiple approaches, pick the best.
    #[allow(clippy::type_complexity)]
    pub fn speculate<'a, T>(
        &'a mut self,
        name: &str,
        arms: Vec<(
            &str,
            std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, CruxErr>> + Send>>,
        )>,
    ) -> crate::speculation::SpeculationBuilder<'a, T>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
    {
        let spec_arms = arms
            .into_iter()
            .map(|(arm_name, fut)| crate::speculation::SpecArm {
                name: arm_name.to_string(),
                fut,
            })
            .collect();
        crate::speculation::SpeculationBuilder::new(self, name, spec_arms)
    }

    /// Apply a `Recovery<serde_json::Value>` and convert back to `T`.
    async fn apply_recovery<T>(
        &mut self,
        name: &str,
        input_hash: u64,
        confidence: f32,
        recovery: Recovery<serde_json::Value>,
    ) -> Result<T, CruxErr>
    where
        T: serde::de::DeserializeOwned,
    {
        let zero_rec = StepRecord {
            name,
            input_hash,
            confidence,
            started_at: Utc::now(),
            duration_ms: 0,
            attempt: 0,
        };

        match recovery {
            Recovery::Continue => {
                if let Some(output) = self.recorder.last_output() {
                    return serde_json::from_value(output.clone()).map_err(|e| {
                        CruxErr::step_failed(name, format!("continue deserialize: {e}"))
                    });
                }
                Err(CruxErr::step_failed(name, "continue with no output"))
            }
            Recovery::Substitute(val) => {
                self.recorder.record_ok(&zero_rec, Some(val.clone()));
                serde_json::from_value(val)
                    .map_err(|e| CruxErr::step_failed(name, format!("substitute deserialize: {e}")))
            }
            Recovery::Skip => {
                self.recorder.record_skipped(name, input_hash, confidence);
                Err(CruxErr::step_failed(name, "step skipped"))
            }
            Recovery::Propagate => {
                if let Some(err_msg) = self.recorder.last_error() {
                    return Err(CruxErr::step_failed(name, err_msg.to_string()));
                }
                Err(CruxErr::step_failed(name, "propagated"))
            }
            Recovery::Escalate(fut) => match fut.await {
                Ok(val) => {
                    let esc_name = format!("{name}::escalated");
                    let esc_rec = StepRecord {
                        name: &esc_name,
                        input_hash,
                        confidence: 1.0,
                        started_at: Utc::now(),
                        duration_ms: 0,
                        attempt: 1,
                    };
                    self.recorder.record_ok(&esc_rec, Some(val.clone()));
                    serde_json::from_value(val).map_err(|e| {
                        CruxErr::step_failed(name, format!("escalate deserialize: {e}"))
                    })
                }
                Err(e) => Err(e),
            },
            Recovery::Retry | Recovery::RetryWith(_) => {
                let retry_name = format!("{name}::retry_requested");
                let retry_rec = StepRecord {
                    name: &retry_name,
                    ..zero_rec
                };
                self.recorder
                    .record_err(&retry_rec, "retry requested but closure consumed");
                Err(CruxErr::step_failed(
                    name,
                    "retry not supported in single-shot step; use step_retryable",
                ))
            }
        }
    }

    /// Core single-shot execution: run closure, check hooks, record step.
    async fn execute_single<F, Fut, T>(
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

        let rec = StepRecord {
            name,
            input_hash,
            confidence,
            started_at: step_start,
            duration_ms,
            attempt: 1,
        };

        match result {
            Ok(val) => {
                if let Some(recovery) = self.hooks.check_confidence(confidence).await {
                    self.recorder
                        .record_ok(&rec, serde_json::to_value(&val).ok());
                    return self
                        .apply_recovery(name, input_hash, confidence, recovery)
                        .await;
                }
                self.recorder
                    .record_ok(&rec, serde_json::to_value(&val).ok());
                Ok(val)
            }
            Err(e) => {
                self.recorder.record_err(&rec, &e.to_string());

                if self.hooks.has_failure_handler() {
                    let recovery = self.hooks.check_failure(e.clone()).await.unwrap();
                    return self
                        .apply_recovery(name, input_hash, confidence, recovery)
                        .await;
                }

                Err(e)
            }
        }
    }
}

impl Context for CruxCtx {
    async fn step<F, Fut, T>(&mut self, name: &str, f: F) -> Result<T, CruxErr>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, CruxErr>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send,
    {
        self.step_with_confidence(name, 1.0, f).await
    }

    async fn step_with_confidence<F, Fut, T>(
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
        let (ordinal, input_hash) = self.recorder.next_ordinal(name);

        // Replay check
        match self.replay.check(ordinal, input_hash) {
            ReplayResult::Hit(cached) => {
                let value: T = deserialize_replay(name, cached.clone())?;
                self.recorder
                    .record_replay(name, input_hash, confidence, cached);
                return Ok(value);
            }
            ReplayResult::Mismatch { expected, actual } => {
                return Err(CruxErr::ReplayMismatch {
                    step: name.to_string(),
                    expected,
                    actual,
                });
            }
            ReplayResult::Miss => {}
        }

        // Budget check
        if self.budget_tracker.is_exceeded() {
            if let Some(recovery) = self
                .hooks
                .check_budget(self.budget_tracker.budget().clone())
                .await
            {
                return self
                    .apply_recovery(name, input_hash, confidence, recovery)
                    .await;
            }
            return Err(CruxErr::BudgetExceeded {
                budget_kind: self.budget_tracker.budget().kind(),
                limit: self.budget_tracker.budget().limit(),
                actual: self.budget_tracker.budget().limit() + 1,
            });
        }

        self.execute_single(name, confidence, input_hash, f).await
    }

    async fn step_retryable<F, Fut, T>(
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
        let (_ordinal, input_hash) = self.recorder.next_ordinal(name);
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

            let rec = StepRecord {
                name,
                input_hash,
                confidence,
                started_at: step_start,
                duration_ms,
                attempt,
            };

            match result {
                Ok(val) => {
                    if let Some(recovery) = self.hooks.check_confidence(confidence).await {
                        self.recorder
                            .record_ok(&rec, serde_json::to_value(&val).ok());
                        match recovery {
                            Recovery::Retry => continue,
                            other => {
                                return self
                                    .apply_recovery(name, input_hash, confidence, other)
                                    .await;
                            }
                        }
                    }
                    self.recorder
                        .record_ok(&rec, serde_json::to_value(&val).ok());
                    return Ok(val);
                }
                Err(e) => {
                    self.recorder.record_err(&rec, &e.to_string());

                    if self.hooks.has_failure_handler() {
                        let recovery = self.hooks.check_failure(e.clone()).await.unwrap();
                        match recovery {
                            Recovery::Retry => continue,
                            Recovery::RetryWith(make_new) => {
                                let retry_start = Utc::now();
                                let retry_result = make_new().await;
                                let retry_dur =
                                    (Utc::now() - retry_start).num_milliseconds().unsigned_abs();
                                match retry_result {
                                    Ok(val) => {
                                        let retry_name = format!("{name}::retry_with");
                                        let retry_rec = StepRecord {
                                            name: &retry_name,
                                            input_hash,
                                            confidence: 1.0,
                                            started_at: retry_start,
                                            duration_ms: retry_dur,
                                            attempt: attempt + 1,
                                        };
                                        self.recorder.record_ok(&retry_rec, Some(val.clone()));
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
                                return self
                                    .apply_recovery(name, input_hash, confidence, other)
                                    .await;
                            }
                        }
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }

    fn on_low_confidence<F, Fut>(&mut self, threshold: f32, handler: F)
    where
        F: Fn(f32) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
    {
        self.hooks.on_low_confidence(threshold, handler);
    }

    fn on_step_failure<F, Fut>(&mut self, handler: F)
    where
        F: Fn(CruxErr) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
    {
        self.hooks.on_step_failure(handler);
    }

    fn on_budget_exceeded<F, Fut>(&mut self, handler: F)
    where
        F: Fn(Budget) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
    {
        self.hooks.on_budget_exceeded(handler);
    }

    fn set_max_retries(&mut self, n: u32) {
        self.max_retries = n;
    }

    fn set_budget(&mut self, budget: Budget) {
        self.budget_tracker = BudgetTracker::new(budget);
    }

    fn consume_budget(&mut self, amount: u64) {
        self.budget_tracker.consume(amount);
    }

    fn budget(&self) -> &Budget {
        self.budget_tracker.budget()
    }

    fn remaining_budget(&self) -> u64 {
        self.budget_tracker.remaining()
    }

    fn step_count(&self) -> u32 {
        self.recorder.current_ordinal()
    }

    fn snapshot_steps(&self) -> &[crate::types::step::Step] {
        self.recorder.steps()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::step::StepStatus;

    #[tokio::test]
    async fn step_records_success() {
        let mut ctx = CruxCtx::new("test_agent");
        let val = ctx.step("greet", || async { Ok(42) }).await.unwrap();
        assert_eq!(val, 42);
        assert_eq!(ctx.snapshot_steps().len(), 1);
        assert_eq!(ctx.snapshot_steps()[0].name, "greet");
        assert_eq!(ctx.snapshot_steps()[0].status, StepStatus::Ok);
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
        assert_eq!(ctx.snapshot_steps()[0].status, StepStatus::Err);
        assert_eq!(
            ctx.snapshot_steps()[0].error.as_deref(),
            Some("step 'fail' failed: boom")
        );
    }

    #[tokio::test]
    async fn finalize_produces_crux() {
        let mut ctx = CruxCtx::new("hello");
        let _: String = ctx
            .step("a", || async { Ok("hi".to_string()) })
            .await
            .unwrap();
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
        assert_ne!(
            ctx.snapshot_steps()[0].input_hash,
            ctx.snapshot_steps()[1].input_hash
        );
    }

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
        assert!(ctx.snapshot_steps().len() >= 2);
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
        let skipped = ctx
            .snapshot_steps()
            .iter()
            .any(|s| s.status == StepStatus::Skipped);
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
        let ok_count = ctx.snapshot_steps().iter().filter(|s| s.is_ok()).count();
        let err_count = ctx.snapshot_steps().iter().filter(|s| s.is_err()).count();
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
        let escalated = ctx
            .snapshot_steps()
            .iter()
            .any(|s| s.name.contains("escalated"));
        assert!(escalated);
    }

    #[tokio::test]
    async fn budget_exceeded_fires_hook() {
        let mut ctx = CruxCtx::new("test");
        ctx.set_budget(Budget::tokens(10));
        ctx.consume_budget(100);
        ctx.on_budget_exceeded(|_budget| async { Recovery::Substitute(serde_json::json!(0)) });
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

    #[tokio::test]
    async fn replay_returns_cached_output() {
        let mut ctx1 = CruxCtx::new("agent");
        let _: String = ctx1
            .step("fetch", || async { Ok("cached_data".to_string()) })
            .await
            .unwrap();
        let crux1 = ctx1.finalize(Ok("done".to_string()));
        let snapshot = crux1.to_snapshot().unwrap();

        let mut ctx2 = CruxCtx::new("agent");
        ctx2.replay_from(&snapshot);
        let val: String = ctx2
            .step("fetch", || async {
                panic!("should not execute during replay")
            })
            .await
            .unwrap();
        assert_eq!(val, "cached_data");
        assert_eq!(ctx2.snapshot_steps()[0].attempt, 0);
    }

    #[tokio::test]
    async fn replay_mismatch_errors() {
        let mut ctx1 = CruxCtx::new("agent");
        let _: String = ctx1
            .step("fetch", || async { Ok("data".to_string()) })
            .await
            .unwrap();
        let crux1 = ctx1.finalize(Ok("done".to_string()));
        let snapshot = crux1.to_snapshot().unwrap();

        let mut ctx2 = CruxCtx::new("agent");
        ctx2.replay_from(&snapshot);
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
        let _ = ctx2.step("a", || async { Ok(1) }).await.unwrap();
        let val = ctx2.step("b", || async { Ok(2) }).await.unwrap();
        assert_eq!(val, 2);
        assert_eq!(ctx2.snapshot_steps().len(), 2);
        assert_eq!(ctx2.snapshot_steps()[0].attempt, 0);
        assert_eq!(ctx2.snapshot_steps()[1].attempt, 1);
    }
}
