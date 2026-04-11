//! CruxCtx — the production adapter for the Context trait.
//!
//! Also re-exports `ConfidenceRange` used by `route_on_confidence`.

/// A half-open or closed confidence range for use with `CruxCtx::route_on_confidence`.
///
/// `lo..hi` is exclusive on the upper end; `lo..=hi` is inclusive.
#[derive(Debug, Clone, Copy)]
pub struct ConfidenceRange {
    lo: f32,
    hi: f32,
    inclusive: bool,
}

impl ConfidenceRange {
    /// Exclusive upper bound: `lo <= x < hi`.
    ///
    /// # Panics
    ///
    /// Panics if either bound is NaN or infinite, if `lo >= hi`, or if either
    /// bound is outside `[0.0, 1.0]`.
    pub fn exclusive(lo: f32, hi: f32) -> Self {
        assert!(
            lo.is_finite() && hi.is_finite(),
            "ConfidenceRange: bounds must be finite (got lo={lo}, hi={hi})"
        );
        assert!(
            lo >= 0.0 && hi <= 1.0,
            "ConfidenceRange: bounds must be in [0.0, 1.0] (got lo={lo}, hi={hi})"
        );
        assert!(
            lo < hi,
            "ConfidenceRange: lo must be strictly less than hi (got lo={lo}, hi={hi})"
        );
        Self {
            lo,
            hi,
            inclusive: false,
        }
    }

    /// Inclusive upper bound: `lo <= x <= hi`.
    ///
    /// # Panics
    ///
    /// Panics if either bound is NaN or infinite, if `lo >= hi`, or if either
    /// bound is outside `[0.0, 1.0]`.
    pub fn inclusive(lo: f32, hi: f32) -> Self {
        assert!(
            lo.is_finite() && hi.is_finite(),
            "ConfidenceRange: bounds must be finite (got lo={lo}, hi={hi})"
        );
        assert!(
            lo >= 0.0 && hi <= 1.0,
            "ConfidenceRange: bounds must be in [0.0, 1.0] (got lo={lo}, hi={hi})"
        );
        assert!(
            lo < hi,
            "ConfidenceRange: lo must be strictly less than hi (got lo={lo}, hi={hi})"
        );
        Self {
            lo,
            hi,
            inclusive: true,
        }
    }

    fn contains(&self, x: f32) -> bool {
        if self.inclusive {
            x >= self.lo && x <= self.hi
        } else {
            x >= self.lo && x < self.hi
        }
    }

    /// Upper end as exclusive for overlap/coverage arithmetic.
    ///
    /// For inclusive ranges we return the next representable `f32` above `hi`
    /// (i.e. `nextafter(hi, +∞)`). This ensures that a contiguous pair such as
    /// `[0.8, 1.0]` and `[0.0, 0.8)` is detected as touching without any
    /// floating-point precision issues, and that inclusive `1.0` correctly
    /// satisfies the `> 1.0` coverage check.
    fn hi_exclusive(&self) -> f32 {
        if self.inclusive {
            // For finite positive floats the bit pattern increments monotonically,
            // so adding 1 to the bit representation gives the next float above hi.
            f32::from_bits(self.hi.to_bits() + 1)
        } else {
            self.hi
        }
    }
}

/// A pinned, boxed, Send future returning `Result<T, CruxErr>`.
pub type BoxFut<T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, CruxErr>> + Send>>;

/// A named route for `route_on_confidence`: (range, label, future).
pub type ConfidenceRoute<'a, T> = (ConfidenceRange, &'a str, BoxFut<T>);

/// A named stage for `pipe`: (label, closure producing a future).
pub type PipeStage<'a, T> = (&'a str, Box<dyn FnOnce(T) -> BoxFut<T> + Send>);

/// A named arm for `join_all`: (label, future).
pub type JoinArm<'a, T> = (&'a str, BoxFut<T>);

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

    /// Set the replay mode (strict or lenient).
    pub fn set_replay_mode(&mut self, mode: crate::replay::ReplayMode) {
        self.replay.set_mode(mode);
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

    /// Route execution based on a confidence score to the first matching named range.
    ///
    /// Each route is a `(ConfidenceRange, label, future)` tuple.
    /// Validates at call time that ranges are non-overlapping and collectively cover `[0.0, 1.0]`.
    /// Returns `Err` if validation fails or no range matches the given confidence.
    pub async fn route_on_confidence<T>(
        &mut self,
        name: &str,
        confidence: f32,
        routes: Vec<ConfidenceRoute<'_, T>>,
    ) -> Result<T, CruxErr>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Send,
    {
        // Validate confidence score.
        if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
            return Err(CruxErr::step_failed(
                name,
                format!(
                    "route_on_confidence: confidence must be finite and in [0.0, 1.0], got {confidence}"
                ),
            ));
        }

        // Extract (lo, hi_exclusive) for validation; sort by lo using total_cmp
        // to handle any remaining non-finite values without silent NaN misordering.
        let mut bounds: Vec<(f32, f32)> = routes
            .iter()
            .map(|(r, _, _)| (r.lo, r.hi_exclusive()))
            .collect();
        bounds.sort_by(|a, b| a.0.total_cmp(&b.0));

        // Check non-overlapping and gap-free (consecutive ranges must be contiguous)
        for i in 1..bounds.len() {
            if bounds[i].0 < bounds[i - 1].1 {
                return Err(CruxErr::step_failed(
                    name,
                    format!("route_on_confidence: overlapping ranges at index {i}"),
                ));
            }
            if bounds[i].0 > bounds[i - 1].1 {
                return Err(CruxErr::step_failed(
                    name,
                    format!(
                        "route_on_confidence: gap between ranges at index {} and {}",
                        i - 1,
                        i
                    ),
                ));
            }
        }

        // Check coverage of [0.0, 1.0]
        let covers_start = bounds.first().map(|b| b.0 <= 0.0).unwrap_or(false);
        let covers_end = bounds.last().map(|b| b.1 > 1.0).unwrap_or(false);
        if !covers_start || !covers_end {
            return Err(CruxErr::step_failed(
                name,
                "route_on_confidence: ranges do not fully cover [0.0, 1.0]",
            ));
        }

        // Find and run the matching route
        for (range, label, fut) in routes {
            if range.contains(confidence) {
                let step_name = format!("{name}::{label}");
                return self.step(&step_name, move || fut).await;
            }
        }

        Err(CruxErr::step_failed(
            name,
            format!("route_on_confidence: no route matched confidence {confidence}"),
        ))
    }

    /// Sequential pipeline: each closure receives the output of the previous one.
    ///
    /// Each stage is recorded as a separate step named `"{name}::{stage_name}"`.
    pub async fn pipe<T>(
        &mut self,
        name: &str,
        input: T,
        stages: Vec<PipeStage<'_, T>>,
    ) -> Result<T, CruxErr>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
    {
        let mut current = input;
        for (stage_name, f) in stages {
            let step_name = format!("{name}::{stage_name}");
            let val = current;
            current = self.step(&step_name, move || f(val)).await?;
        }
        Ok(current)
    }

    /// Parallel fan-out: run named futures concurrently, record each as a step.
    ///
    /// Ordinals are allocated for all arms **before** any future is dispatched,
    /// matching the semantics of `step()` and `pipe()`. Each arm also checks the
    /// replay cache before executing: if a cached result exists the future is
    /// dropped and the cached value is used directly.
    ///
    /// Each arm is recorded as a step named `"{name}::{arm_name}"`. Returns a
    /// `Vec` of results in input order.
    ///
    /// # Replay semantics
    ///
    /// Unlike bare `step()`, arms whose futures are not in the replay cache are
    /// still dispatched concurrently. Only arms that hit the cache skip execution.
    /// This means a partial replay (some arms cached, some not) is supported, but
    /// the non-cached arms will execute live even during an otherwise-replaying
    /// run. Callers that need strict all-or-nothing replay should use sequential
    /// `step()` calls instead.
    pub async fn join_all<T>(
        &mut self,
        name: &str,
        arms: Vec<JoinArm<'_, T>>,
    ) -> Result<Vec<T>, CruxErr>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
    {
        use chrono::Utc;

        // Phase 1: allocate ordinals and check replay cache for each arm before
        // dispatching any future. This mirrors step()/pipe() ordinal-first semantics.
        struct ArmMeta {
            step_name: String,
            input_hash: u64,
        }

        let mut metas: Vec<ArmMeta> = Vec::with_capacity(arms.len());
        let mut replay_hits: Vec<Option<T>> = Vec::with_capacity(arms.len());

        for (arm_name, _) in &arms {
            let step_name = format!("{name}::{arm_name}");
            let (ordinal, input_hash) = self.recorder.next_ordinal(&step_name);

            let hit = match self.replay.check_by_name(&step_name, ordinal, input_hash) {
                ReplayResult::Hit(cached) => {
                    let value: T = deserialize_replay(&step_name, cached.clone())?;
                    self.recorder
                        .record_replay(&step_name, input_hash, 1.0, cached);
                    Some(value)
                }
                ReplayResult::Mismatch { expected, actual } => {
                    return Err(CruxErr::ReplayMismatch {
                        step: step_name.clone(),
                        expected,
                        actual,
                    });
                }
                ReplayResult::Miss => None,
            };

            let _ = ordinal; // allocated to advance the ordinal counter; not stored
            metas.push(ArmMeta {
                step_name,
                input_hash,
            });
            replay_hits.push(hit);
        }

        // Phase 2: dispatch only the futures whose arms missed the replay cache.
        // Futures for cache-hit arms are dropped here (never polled).
        let live_futs: Vec<(usize, _)> = arms
            .into_iter()
            .enumerate()
            .filter(|(i, _)| replay_hits[*i].is_none())
            .map(|(i, (_, fut))| (i, fut))
            .collect();

        let live_indices: Vec<usize> = live_futs.iter().map(|(i, _)| *i).collect();
        let futs_only: Vec<_> = live_futs.into_iter().map(|(_, f)| f).collect();

        let outcomes: Vec<(chrono::DateTime<Utc>, u64, Result<T, CruxErr>)> =
            futures::future::join_all(futs_only.into_iter().map(|fut| async move {
                let start = Utc::now();
                let result = fut.await;
                let duration_ms = (Utc::now() - start).num_milliseconds().unsigned_abs();
                (start, duration_ms, result)
            }))
            .await;

        // Phase 3: record live outcomes back against pre-allocated ordinal metadata.
        let mut live_outcome_iter = outcomes.into_iter();

        let mut results: Vec<Option<T>> = replay_hits;
        for idx in live_indices {
            let (started_at, duration_ms, result) = live_outcome_iter.next().unwrap();
            let step_name = &metas[idx].step_name;
            let input_hash = metas[idx].input_hash;

            match result {
                Ok(val) => {
                    let rec = crate::recorder::StepRecord {
                        name: step_name,
                        input_hash,
                        confidence: 1.0,
                        started_at,
                        duration_ms,
                        attempt: 1,
                    };
                    self.recorder
                        .record_ok(&rec, serde_json::to_value(&val).ok());
                    results[idx] = Some(val);
                }
                Err(e) => {
                    let rec = crate::recorder::StepRecord {
                        name: step_name,
                        input_hash,
                        confidence: 1.0,
                        started_at,
                        duration_ms,
                        attempt: 1,
                    };
                    self.recorder.record_err(&rec, &e.to_string());
                    return Err(e);
                }
            }
        }

        // All slots filled — unwrap is safe.
        Ok(results.into_iter().map(Option::unwrap).collect())
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

        // Replay check (by-name for better matching in both modes).
        match self.replay.check_by_name(name, ordinal, input_hash) {
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

    // -- route_on_confidence --------------------------------------------------

    fn three_way_routes(
        low_val: String,
        med_val: String,
        high_val: String,
    ) -> Vec<ConfidenceRoute<'static, String>> {
        vec![
            (
                ConfidenceRange::exclusive(0.0, 0.5),
                "low",
                Box::pin(async move { Ok(low_val) }),
            ),
            (
                ConfidenceRange::exclusive(0.5, 0.8),
                "medium",
                Box::pin(async move { Ok(med_val) }),
            ),
            (
                ConfidenceRange::inclusive(0.8, 1.0),
                "high",
                Box::pin(async move { Ok(high_val) }),
            ),
        ]
    }

    #[tokio::test]
    async fn route_on_confidence_routes_correctly() {
        let mut ctx = CruxCtx::new("test");
        let result: String = ctx
            .route_on_confidence(
                "classify",
                0.6,
                three_way_routes("low".into(), "medium".into(), "high".into()),
            )
            .await
            .unwrap();
        assert_eq!(result, "medium");
        assert!(
            ctx.snapshot_steps()
                .iter()
                .any(|s| s.name == "classify::medium")
        );
    }

    #[tokio::test]
    async fn route_on_confidence_low_boundary() {
        let mut ctx = CruxCtx::new("test");
        let result: String = ctx
            .route_on_confidence(
                "r",
                0.1,
                three_way_routes("low".into(), "medium".into(), "high".into()),
            )
            .await
            .unwrap();
        assert_eq!(result, "low");
    }

    #[tokio::test]
    async fn route_on_confidence_rejects_gap() {
        let mut ctx = CruxCtx::new("test");
        // Gap between 0.5 and 0.6 — doesn't cover [0.0, 1.0]
        let result: Result<String, _> = ctx
            .route_on_confidence(
                "r",
                0.3,
                vec![
                    (
                        ConfidenceRange::exclusive(0.0, 0.5),
                        "low",
                        Box::pin(async { Ok("x".to_string()) })
                            as std::pin::Pin<
                                Box<
                                    dyn std::future::Future<Output = Result<String, CruxErr>>
                                        + Send,
                                >,
                            >,
                    ),
                    (
                        ConfidenceRange::inclusive(0.6, 1.0),
                        "high",
                        Box::pin(async { Ok("x".to_string()) }),
                    ),
                ],
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn route_on_confidence_rejects_overlap() {
        let mut ctx = CruxCtx::new("test");
        // 0.0..0.6 and 0.5..=1.0 overlap
        let result: Result<String, _> = ctx
            .route_on_confidence(
                "r",
                0.3,
                vec![
                    (
                        ConfidenceRange::exclusive(0.0, 0.6),
                        "a",
                        Box::pin(async { Ok("x".to_string()) })
                            as std::pin::Pin<
                                Box<
                                    dyn std::future::Future<Output = Result<String, CruxErr>>
                                        + Send,
                                >,
                            >,
                    ),
                    (
                        ConfidenceRange::inclusive(0.5, 1.0),
                        "b",
                        Box::pin(async { Ok("x".to_string()) }),
                    ),
                ],
            )
            .await;
        assert!(result.is_err());
    }

    // -- pipe -----------------------------------------------------------------

    #[tokio::test]
    async fn pipe_chains_stages() {
        let mut ctx = CruxCtx::new("test");
        let result: i32 = ctx
            .pipe(
                "transform",
                1_i32,
                vec![
                    (
                        "double",
                        Box::new(|v: i32| {
                            Box::pin(async move { Ok(v * 2) })
                                as std::pin::Pin<
                                    Box<
                                        dyn std::future::Future<Output = Result<i32, CruxErr>>
                                            + Send,
                                    >,
                                >
                        })
                            as Box<
                                dyn FnOnce(
                                        i32,
                                    ) -> std::pin::Pin<
                                        Box<
                                            dyn std::future::Future<Output = Result<i32, CruxErr>>
                                                + Send,
                                        >,
                                    > + Send,
                            >,
                    ),
                    (
                        "add_ten",
                        Box::new(|v: i32| Box::pin(async move { Ok(v + 10) })),
                    ),
                ],
            )
            .await
            .unwrap();
        assert_eq!(result, 12); // (1*2)+10
        assert_eq!(ctx.snapshot_steps().len(), 2);
        assert_eq!(ctx.snapshot_steps()[0].name, "transform::double");
        assert_eq!(ctx.snapshot_steps()[1].name, "transform::add_ten");
    }

    #[tokio::test]
    async fn pipe_short_circuits_on_error() {
        let mut ctx = CruxCtx::new("test");
        let result: Result<i32, _> = ctx
            .pipe(
                "p",
                0_i32,
                vec![
                    (
                        "fail",
                        Box::new(|_v: i32| {
                            Box::pin(async { Err(CruxErr::step_failed("fail", "bad")) })
                                as std::pin::Pin<
                                    Box<
                                        dyn std::future::Future<Output = Result<i32, CruxErr>>
                                            + Send,
                                    >,
                                >
                        })
                            as Box<
                                dyn FnOnce(
                                        i32,
                                    ) -> std::pin::Pin<
                                        Box<
                                            dyn std::future::Future<Output = Result<i32, CruxErr>>
                                                + Send,
                                        >,
                                    > + Send,
                            >,
                    ),
                    (
                        "unreachable",
                        Box::new(|v: i32| Box::pin(async move { Ok(v + 1) })),
                    ),
                ],
            )
            .await;
        assert!(result.is_err());
        assert_eq!(ctx.snapshot_steps().len(), 1);
    }

    // -- join_all -------------------------------------------------------------

    #[tokio::test]
    async fn join_all_runs_concurrently_and_collects() {
        let mut ctx = CruxCtx::new("test");
        let results: Vec<i32> = ctx
            .join_all(
                "fetch",
                vec![
                    ("a", Box::pin(async { Ok(1_i32) })),
                    ("b", Box::pin(async { Ok(2_i32) })),
                    ("c", Box::pin(async { Ok(3_i32) })),
                ],
            )
            .await
            .unwrap();
        assert_eq!(results, vec![1, 2, 3]);
        assert_eq!(ctx.snapshot_steps().len(), 3);
        assert_eq!(ctx.snapshot_steps()[0].name, "fetch::a");
        assert_eq!(ctx.snapshot_steps()[1].name, "fetch::b");
        assert_eq!(ctx.snapshot_steps()[2].name, "fetch::c");
    }

    // -- ConfidenceRange validation -------------------------------------------

    #[test]
    #[should_panic(expected = "bounds must be finite")]
    fn confidence_range_exclusive_rejects_nan() {
        ConfidenceRange::exclusive(f32::NAN, 1.0);
    }

    #[test]
    #[should_panic(expected = "bounds must be finite")]
    fn confidence_range_exclusive_rejects_inf() {
        ConfidenceRange::exclusive(0.0, f32::INFINITY);
    }

    #[test]
    #[should_panic(expected = "lo must be strictly less than hi")]
    fn confidence_range_exclusive_rejects_reversed() {
        ConfidenceRange::exclusive(0.8, 0.2);
    }

    #[test]
    #[should_panic(expected = "lo must be strictly less than hi")]
    fn confidence_range_exclusive_rejects_equal() {
        ConfidenceRange::exclusive(0.5, 0.5);
    }

    #[test]
    #[should_panic(expected = "bounds must be in [0.0, 1.0]")]
    fn confidence_range_exclusive_rejects_out_of_range() {
        ConfidenceRange::exclusive(-0.1, 0.5);
    }

    #[test]
    #[should_panic(expected = "bounds must be finite")]
    fn confidence_range_inclusive_rejects_nan() {
        ConfidenceRange::inclusive(0.0, f32::NAN);
    }

    #[test]
    #[should_panic(expected = "bounds must be in [0.0, 1.0]")]
    fn confidence_range_inclusive_rejects_out_of_range() {
        ConfidenceRange::inclusive(0.5, 1.1);
    }

    #[tokio::test]
    async fn route_on_confidence_rejects_nan_confidence() {
        let mut ctx = CruxCtx::new("test");
        let result: Result<String, _> = ctx
            .route_on_confidence(
                "r",
                f32::NAN,
                three_way_routes("a".into(), "b".into(), "c".into()),
            )
            .await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("confidence must be finite"));
    }

    #[tokio::test]
    async fn route_on_confidence_rejects_infinite_confidence() {
        let mut ctx = CruxCtx::new("test");
        let result: Result<String, _> = ctx
            .route_on_confidence(
                "r",
                f32::INFINITY,
                three_way_routes("a".into(), "b".into(), "c".into()),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn route_on_confidence_rejects_out_of_range_confidence() {
        let mut ctx = CruxCtx::new("test");
        let result: Result<String, _> = ctx
            .route_on_confidence(
                "r",
                1.5,
                three_way_routes("a".into(), "b".into(), "c".into()),
            )
            .await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("confidence must be finite"));
    }

    // -- join_all replay semantics --------------------------------------------

    #[tokio::test]
    async fn join_all_replays_from_cache() {
        // Record a first run.
        let mut ctx1 = CruxCtx::new("agent");
        let results1: Vec<String> = ctx1
            .join_all(
                "fetch",
                vec![
                    ("a", Box::pin(async { Ok("alpha".to_string()) })),
                    ("b", Box::pin(async { Ok("beta".to_string()) })),
                ],
            )
            .await
            .unwrap();
        assert_eq!(results1, vec!["alpha", "beta"]);
        let crux1 = ctx1.finalize(Ok("done".to_string()));
        let snapshot = crux1.to_snapshot().unwrap();

        // Replay: futures must not execute.
        let mut ctx2 = CruxCtx::new("agent");
        ctx2.replay_from(&snapshot);
        let results2: Vec<String> = ctx2
            .join_all(
                "fetch",
                vec![
                    (
                        "a",
                        Box::pin(async { panic!("should not run during replay") }),
                    ),
                    (
                        "b",
                        Box::pin(async { panic!("should not run during replay") }),
                    ),
                ],
            )
            .await
            .unwrap();
        assert_eq!(results2, vec!["alpha", "beta"]);
        // Replayed steps have attempt == 0.
        assert_eq!(ctx2.snapshot_steps()[0].attempt, 0);
        assert_eq!(ctx2.snapshot_steps()[1].attempt, 0);
    }

    #[tokio::test]
    async fn join_all_ordinals_allocated_before_dispatch() {
        // Verify step names and count are correct even after a join_all.
        let mut ctx = CruxCtx::new("test");
        let _ = ctx
            .join_all(
                "fan",
                vec![
                    ("x", Box::pin(async { Ok(10_i32) })),
                    ("y", Box::pin(async { Ok(20_i32) })),
                ],
            )
            .await
            .unwrap();
        // A step recorded after join_all must have ordinal 3 (after the two arms).
        let _ = ctx.step("post", || async { Ok(99_i32) }).await.unwrap();
        assert_eq!(ctx.step_count(), 3);
        assert_eq!(ctx.snapshot_steps()[2].name, "post");
    }

    #[tokio::test]
    async fn join_all_returns_first_error() {
        let mut ctx = CruxCtx::new("test");
        let result: Result<Vec<i32>, _> = ctx
            .join_all(
                "fetch",
                vec![
                    ("ok", Box::pin(async { Ok(1_i32) })),
                    (
                        "bad",
                        Box::pin(async { Err(CruxErr::step_failed("bad", "oops")) }),
                    ),
                ],
            )
            .await;
        assert!(result.is_err());
    }
}
