//! CruxCtx — the production adapter for the Context trait.
//!
//! Also re-exports `ConfidenceRange` used by `route_on_confidence`.

// TODO(#72): planner-based action dispatch — refactor step/delegate/speculate to return
//   abstract Action variants (CallProvider | ExecuteTool | Finish) enabling dry-run,
//   simulation, and side-effect-free testing

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

use crux_domain::event::StepEvent;
use crux_domain::pipeline::EventSender;
use crux_domain::plan_result::PlanResult;
use crux_domain::planner::{PassthroughPlanner, Planner};

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

const DEFAULT_MAX_RETRIES: u32 = 3;

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
    pub(crate) planner: std::sync::Arc<dyn Planner>,
    event_sender: Option<EventSender>,
    state: std::sync::Arc<std::sync::RwLock<crux_types::step::StepState>>,
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
            max_retries: DEFAULT_MAX_RETRIES,
            planner: std::sync::Arc::new(PassthroughPlanner),
            event_sender: None,
            state: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Register a pre-step safety gate. If the gate returns `Deny`, the step
    /// is recorded as skipped and a `CruxErr::Denied` is returned.
    pub fn on_pre_step<F>(&mut self, gate: F)
    where
        F: Fn(&str) -> crate::hooks::HookVerdict + Send + Sync + 'static,
    {
        self.hooks.on_pre_step(gate);
    }

    /// Attach a redactor that scrubs output/error fields before they enter the
    /// trace. Applies to all subsequent steps.
    pub fn set_redactor(&mut self, redactor: Box<dyn crate::recorder::Redactor>) {
        self.recorder.set_redactor(redactor);
    }

    /// Store the output of a named pipe stage so later stages can read it.
    pub fn propagate_output(&self, alias: &str, output: serde_json::Value) {
        let mut state = self
            .state
            .write()
            .expect("StepState RwLock poisoned — irrecoverable");
        state.insert(alias.to_string(), output);
    }

    /// Retrieve a previously propagated output by alias name.
    ///
    /// Returns `None` if no output has been stored under `alias`.
    pub fn read_output(&self, alias: &str) -> Option<serde_json::Value> {
        let state = self
            .state
            .read()
            .expect("StepState RwLock poisoned — irrecoverable");
        state.get(alias).cloned()
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        Self::new("__test__")
    }

    /// Set the planner for this context. The planner is consulted before each step.
    pub fn set_planner(&mut self, planner: impl Planner + 'static) {
        self.planner = std::sync::Arc::new(planner);
    }

    /// Set a pre-boxed Arc planner (used internally for child context propagation).
    pub(crate) fn set_planner_arc(&mut self, planner: std::sync::Arc<dyn Planner>) {
        self.planner = planner;
    }

    /// Attach an event sender so this context emits `StepEvent`s on every step.
    pub fn set_event_sender(&mut self, sender: EventSender) {
        self.event_sender = Some(sender);
    }

    /// Emit a step event to the attached sender, if any.
    fn emit(&self, event: StepEvent) {
        if let Some(ref tx) = self.event_sender {
            let _ = tx.send(event);
        }
    }

    /// Emit an intermediate event for a named step.
    ///
    /// Broadcasts via the EventPipeline (if attached) as a `StepEvent::Chunk`.
    pub fn emit_step_event(&self, step_name: &str, payload: serde_json::Value) {
        self.emit(StepEvent::Chunk {
            step_name: step_name.to_string(),
            payload,
        });
    }

    /// Seed replay from a previous trace.
    pub fn replay_from(&mut self, previous: &Crux<serde_json::Value>) {
        self.replay.seed_from(previous);
    }

    /// Set the replay mode (strict or lenient).
    pub fn set_replay_mode(&mut self, mode: crate::replay::ReplayMode) {
        self.replay.set_mode(mode);
    }

    /// Take a mid-run checkpoint: snapshot the current trace into a `Crux<Value>`.
    ///
    /// The snapshot can be persisted to a `TaskRegistry` and later used to
    /// resume execution via `replay_from`.
    pub fn snapshot(&self) -> Crux<serde_json::Value> {
        Crux {
            id: self.id.clone(),
            agent: self.agent_name.clone(),
            value: Ok(serde_json::Value::Null),
            steps: self.recorder.steps().to_vec(),
            children: self.children.clone(),
            started_at: self.started_at,
            finished_at: None,
        }
    }

    /// Checkpoint current trace to a TaskRegistry.
    ///
    /// Serializes the in-progress trace and stores it as the task's checkpoint.
    /// On resume, call `replay_from` with the loaded checkpoint.
    pub async fn checkpoint_to<B: crate::registry::RegistryBackend>(
        &self,
        registry: &crate::registry::TaskRegistry<B>,
        task_id: &crate::types::id::TaskId,
    ) -> Result<(), crate::registry::RegistryErr> {
        let snapshot = self.snapshot();
        registry.checkpoint(task_id, &snapshot).await
    }

    /// Resume from a previously checkpointed task.
    ///
    /// Loads the checkpoint from the registry and seeds the replay cache.
    /// Steps that were already completed will be replayed from cache.
    pub async fn resume_from<B: crate::registry::RegistryBackend>(
        &mut self,
        registry: &crate::registry::TaskRegistry<B>,
        task_id: &crate::types::id::TaskId,
    ) -> Result<(), CruxErr> {
        let checkpoint = registry
            .load_checkpoint(task_id)
            .await
            .map_err(|e| CruxErr::step_failed("resume", e.to_string()))?;
        if let Some(cp) = checkpoint {
            self.replay_from(&cp);
        }
        Ok(())
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

    // -- Internal helpers for orchestration --------------------------

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
            content_hash: None,
            output,
            error,
            attempt: 1,
            events: vec![],
            metadata: std::collections::HashMap::new(),
            findings: vec![],
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
                trace_route!(name, confidence, label);
                let step_name = format!("{name}::{label}");
                let val = self.step(&step_name, move || fut).await?;
                // If the handler output is a JSON object with a "confidence" field
                // (finite f32 in [0.0, 1.0]), propagate it to the recorded step so
                // that route_on_confidence can act on meaningful handler-reported scores.
                if let Ok(json) = serde_json::to_value(&val)
                    && let Some(handler_conf) = json
                        .get("confidence")
                        .and_then(|v| v.as_f64())
                        .map(|f| f as f32)
                        .filter(|f| f.is_finite() && (0.0..=1.0).contains(f))
                    && let Some(step) = self.recorder.steps_mut().last_mut()
                {
                    step.confidence = handler_conf;
                }
                return Ok(val);
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
        trace_pipe!(name, stages.len());
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

        trace_join_all!(name, arms.len());

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

            let hit = match self
                .replay
                .check_by_name(&step_name, ordinal, input_hash, None)
            {
                ReplayResult::Hit(cached) => {
                    let value: T = deserialize_replay(&step_name, cached.clone())?;
                    self.recorder
                        .record_replay(&step_name, input_hash, None, 1.0, cached);
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

        // Budget check — mirrors step_with_confidence.
        if self.budget_tracker.is_exceeded() {
            if let Some(recovery) = self
                .hooks
                .check_budget(self.budget_tracker.budget().clone())
                .await
            {
                // Use the first arm's metadata for recovery recording.
                let meta = &metas[0];
                return self
                    .apply_recovery::<Vec<T>>(name, meta.input_hash, 1.0, recovery)
                    .await;
            }
            return Err(CruxErr::BudgetExceeded {
                budget_kind: self.budget_tracker.budget().kind(),
                limit: self.budget_tracker.budget().limit(),
                actual: self.budget_tracker.budget().limit() + 1,
            });
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
                        content_hash: None,
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
                        content_hash: None,
                        confidence: 1.0,
                        started_at,
                        duration_ms,
                        attempt: 1,
                    };
                    self.recorder.record_err(&rec, &e.to_string());

                    // Consult on_step_failure hook (mirrors execute_single).
                    if let Some(recovery) = self.hooks.check_failure(e.clone()).await {
                        return self
                            .apply_recovery(step_name, input_hash, 1.0, recovery)
                            .await;
                    }

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
            content_hash: None,
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
                        content_hash: None,
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
        content_hash: Option<u64>,
        f: F,
    ) -> Result<T, CruxErr>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, CruxErr>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send,
    {
        self.emit(StepEvent::Started {
            step_name: name.to_string(),
        });
        let step_start = Utc::now();
        let result = f().await;
        let duration_ms = (Utc::now() - step_start).num_milliseconds().unsigned_abs();

        let rec = StepRecord {
            name,
            input_hash,
            content_hash,
            confidence,
            started_at: step_start,
            duration_ms,
            attempt: 1,
        };

        match result {
            Ok(val) => {
                if let Some(recovery) = self.hooks.check_confidence(confidence).await {
                    trace_hook!("on_low_confidence", name);
                    self.recorder
                        .record_ok(&rec, serde_json::to_value(&val).ok());
                    return self
                        .apply_recovery(name, input_hash, confidence, recovery)
                        .await;
                }
                self.recorder
                    .record_ok(&rec, serde_json::to_value(&val).ok());
                self.emit(StepEvent::Completed {
                    step_name: name.to_string(),
                    duration_ms,
                });
                Ok(val)
            }
            Err(e) => {
                self.recorder.record_err(&rec, &e.to_string());
                self.emit(StepEvent::Failed {
                    step_name: name.to_string(),
                    error: e.to_string(),
                });

                if let Some(recovery) = self.hooks.check_failure(e.clone()).await {
                    trace_hook!("on_step_failure", name);
                    return self
                        .apply_recovery(name, input_hash, confidence, recovery)
                        .await;
                }

                Err(e)
            }
        }
    }
}

impl CruxCtx {
    async fn step_inner<F, Fut, T>(
        &mut self,
        name: &str,
        confidence: f32,
        content_hash: Option<u64>,
        f: F,
    ) -> Result<T, CruxErr>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, CruxErr>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send,
    {
        trace_step!(name, confidence);

        // Planner check — before replay cache and closure execution.
        match self.planner.next_action(name, 0) {
            PlanResult::Deny { reason } => {
                return Err(CruxErr::Denied {
                    step: name.to_string(),
                    reason,
                });
            }
            PlanResult::Simulate { output } => {
                return serde_json::from_value(output)
                    .map_err(|e| CruxErr::step_failed(name, e.to_string()));
            }
            PlanResult::Allow(_) => {} // proceed normally
        }

        // Pre-step safety gate
        if let crate::hooks::HookVerdict::Deny(reason) = self.hooks.check_pre_step(name) {
            let (_, input_hash) = self.recorder.next_ordinal(name);
            self.recorder.record_skipped(name, input_hash, confidence);
            return Err(CruxErr::Denied {
                step: name.to_string(),
                reason,
            });
        }

        let (ordinal, input_hash) = self.recorder.next_ordinal(name);

        // Replay check (by-name for better matching in both modes).
        match self
            .replay
            .check_by_name(name, ordinal, input_hash, content_hash)
        {
            ReplayResult::Hit(cached) => {
                trace_replay_hit!(name);
                let value: T = deserialize_replay(name, cached.clone())?;
                self.recorder
                    .record_replay(name, input_hash, content_hash, confidence, cached);
                return Ok(value);
            }
            ReplayResult::Mismatch { expected, actual } => {
                return Err(CruxErr::ReplayMismatch {
                    step: name.to_string(),
                    expected,
                    actual,
                });
            }
            ReplayResult::Miss => {
                trace_replay_miss!(name);
            }
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

        self.execute_single(name, confidence, input_hash, content_hash, f)
            .await
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

    async fn step_keyed<F, Fut, T, K>(&mut self, name: &str, key: &K, f: F) -> Result<T, CruxErr>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, CruxErr>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send,
        K: serde::Serialize + Sync,
    {
        let content_hash = Some(crate::recorder::hash_content(key));
        self.step_inner(name, 1.0, content_hash, f).await
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
        self.step_inner(name, confidence, None, f).await
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
                content_hash: None,
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

                    if let Some(recovery) = self.hooks.check_failure(e.clone()).await {
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
                                            content_hash: None,
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

    async fn step_stream<F, S, T>(&mut self, name: &str, f: F) -> Result<T, CruxErr>
    where
        F: FnOnce() -> S + Send,
        S: futures::Stream<Item = Result<T, CruxErr>> + Send + Unpin,
        T: serde::Serialize + serde::de::DeserializeOwned + Send,
    {
        use futures::StreamExt;

        trace_step!(name, 1.0_f32);
        let (ordinal, input_hash) = self.recorder.next_ordinal(name);

        // Replay check
        match self.replay.check_by_name(name, ordinal, input_hash, None) {
            ReplayResult::Hit(cached) => {
                trace_replay_hit!(name);
                let value: T = deserialize_replay(name, cached.clone())?;
                self.recorder
                    .record_replay(name, input_hash, None, 1.0, cached);
                return Ok(value);
            }
            ReplayResult::Mismatch { expected, actual } => {
                return Err(CruxErr::ReplayMismatch {
                    step: name.to_string(),
                    expected,
                    actual,
                });
            }
            ReplayResult::Miss => {
                trace_replay_miss!(name);
            }
        }

        let step_start = Utc::now();
        let mut stream = f();
        let mut events: Vec<serde_json::Value> = Vec::new();
        let mut last_value: Option<T> = None;

        while let Some(item) = stream.next().await {
            match item {
                Ok(val) => {
                    if let Ok(json_val) = serde_json::to_value(&val) {
                        events.push(json_val);
                    }
                    last_value = Some(val);
                }
                Err(e) => {
                    let duration_ms = (Utc::now() - step_start).num_milliseconds().unsigned_abs();
                    let rec = StepRecord {
                        name,
                        input_hash,
                        content_hash: None,
                        confidence: 1.0,
                        started_at: step_start,
                        duration_ms,
                        attempt: 1,
                    };
                    self.recorder.record_err(&rec, &e.to_string());
                    // Patch in events on the last step
                    if let Some(step) = self.recorder.steps_mut().last_mut() {
                        step.events = events;
                    }
                    return Err(e);
                }
            }
        }

        let duration_ms = (Utc::now() - step_start).num_milliseconds().unsigned_abs();

        match last_value {
            Some(val) => {
                let rec = StepRecord {
                    name,
                    input_hash,
                    content_hash: None,
                    confidence: 1.0,
                    started_at: step_start,
                    duration_ms,
                    attempt: 1,
                };
                self.recorder
                    .record_ok(&rec, serde_json::to_value(&val).ok());
                // Patch in events on the last step
                if let Some(step) = self.recorder.steps_mut().last_mut() {
                    step.events = events;
                }
                Ok(val)
            }
            None => {
                let rec = StepRecord {
                    name,
                    input_hash,
                    content_hash: None,
                    confidence: 1.0,
                    started_at: step_start,
                    duration_ms,
                    attempt: 1,
                };
                self.recorder.record_err(&rec, "stream yielded no items");
                Err(CruxErr::step_failed(name, "stream yielded no items"))
            }
        }
    }

    async fn try_step<F, Fut, T, E>(&mut self, name: &str, f: F) -> Result<T, CruxErr>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, E>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send,
        E: std::fmt::Display + Send,
    {
        let step_name = name.to_string();
        self.step(name, || async move {
            f().await
                .map_err(|e| CruxErr::step_failed(&step_name, e.to_string()))
        })
        .await
    }
}

/// Determines the worst-case outcome across all steps.
/// Steps flagged `continue_on_error` are excluded unless `ignore_continue_on_error` is true.
pub fn determine_final_phase(
    steps: &[crux_types::crux_value::StepRecord],
    ignore_continue_on_error: bool,
) -> crux_types::crux_value::FinalPhase {
    use crux_types::crux_value::FinalPhase;
    steps
        .iter()
        .filter(|s| ignore_continue_on_error || !s.continue_on_error)
        .map(|s| s.phase)
        .max()
        .unwrap_or(FinalPhase::Succeeded)
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
    async fn try_step_converts_arbitrary_error() {
        let mut ctx = CruxCtx::new("test_agent");
        let result: Result<i32, _> = ctx
            .try_step("parse", || async {
                "not_a_number".parse::<i32>() // returns Result<i32, ParseIntError>
            })
            .await;
        assert!(result.is_err());
        assert_eq!(ctx.snapshot_steps()[0].status, StepStatus::Err);
        let err_msg = ctx.snapshot_steps()[0].error.as_deref().unwrap();
        assert!(err_msg.contains("parse"), "error: {err_msg}");
    }

    #[tokio::test]
    async fn try_step_success_records_normally() {
        let mut ctx = CruxCtx::new("test_agent");
        let val: i32 = ctx
            .try_step("double", || async { Ok::<_, std::fmt::Error>(42) })
            .await
            .unwrap();
        assert_eq!(val, 42);
        assert_eq!(ctx.snapshot_steps()[0].status, StepStatus::Ok);
    }

    #[tokio::test]
    async fn pre_step_gate_blocks_denied_step() {
        use crate::hooks::HookVerdict;
        let mut ctx = CruxCtx::new("test_agent");
        ctx.on_pre_step(|name| {
            if name == "forbidden" {
                HookVerdict::Deny("not allowed".into())
            } else {
                HookVerdict::Allow
            }
        });
        // Allowed step succeeds
        let val: i32 = ctx.step("ok_step", || async { Ok(42) }).await.unwrap();
        assert_eq!(val, 42);
        // Denied step fails with Denied error and is recorded as Skipped
        let result: Result<i32, _> = ctx.step("forbidden", || async { Ok(99) }).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, CruxErr::Denied { .. }));
        assert_eq!(ctx.snapshot_steps()[1].status, StepStatus::Skipped);
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

    // -- Issue #8: route_on_confidence confidence field propagation -----------

    #[tokio::test]
    async fn route_on_confidence_propagates_confidence_from_handler_output() {
        // Handler returns a JSON object with a "confidence" field.
        // The recorded step's confidence should reflect that value, not 1.0.
        let mut ctx = CruxCtx::new("test");
        let routes: Vec<ConfidenceRoute<'static, serde_json::Value>> = vec![
            (
                ConfidenceRange::exclusive(0.0, 0.5),
                "low",
                Box::pin(async { Ok(serde_json::json!({"result": "low", "confidence": 0.2})) }),
            ),
            (
                ConfidenceRange::inclusive(0.5, 1.0),
                "high",
                Box::pin(async { Ok(serde_json::json!({"result": "high", "confidence": 0.9})) }),
            ),
        ];
        let _ = ctx
            .route_on_confidence("classify", 0.7, routes)
            .await
            .unwrap();

        // The step for the matched route should record confidence from the output.
        let step = ctx
            .snapshot_steps()
            .iter()
            .find(|s| s.name == "classify::high")
            .expect("classify::high step not found");
        assert!(
            (step.confidence - 0.9).abs() < 1e-6,
            "expected confidence 0.9 from handler output, got {}",
            step.confidence
        );
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

    #[tokio::test]
    async fn join_all_checks_budget_before_dispatch() {
        let mut ctx = CruxCtx::new("test");
        ctx.set_budget(Budget::tokens(10));
        ctx.consume_budget(100);
        let result: Result<Vec<i32>, _> = ctx
            .join_all(
                "fetch",
                vec![
                    ("a", Box::pin(async { panic!("should not run") })),
                    ("b", Box::pin(async { panic!("should not run") })),
                ],
            )
            .await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("budget exceeded"));
    }

    #[tokio::test]
    async fn join_all_budget_hook_fires_substitute() {
        let mut ctx = CruxCtx::new("test");
        ctx.set_budget(Budget::tokens(10));
        ctx.consume_budget(100);
        ctx.on_budget_exceeded(|_budget| async {
            Recovery::Substitute(serde_json::json!([10, 20]))
        });
        let result: Vec<i32> = ctx
            .join_all(
                "fetch",
                vec![
                    ("a", Box::pin(async { panic!("should not run") })),
                    ("b", Box::pin(async { panic!("should not run") })),
                ],
            )
            .await
            .unwrap();
        assert_eq!(result, vec![10, 20]);
    }

    #[tokio::test]
    async fn join_all_failure_hook_substitute() {
        let mut ctx = CruxCtx::new("test");
        ctx.on_step_failure(|_err| async { Recovery::Substitute(serde_json::json!([99])) });
        let result: Vec<i32> = ctx
            .join_all(
                "fetch",
                vec![(
                    "bad",
                    Box::pin(async { Err(CruxErr::step_failed("bad", "oops")) }),
                )],
            )
            .await
            .unwrap();
        assert_eq!(result, vec![99]);
    }

    #[tokio::test]
    async fn join_all_failure_hook_propagate() {
        let mut ctx = CruxCtx::new("test");
        ctx.on_step_failure(|_err| async { Recovery::Propagate });
        let result: Result<Vec<i32>, _> = ctx
            .join_all(
                "fetch",
                vec![(
                    "bad",
                    Box::pin(async { Err(CruxErr::step_failed("bad", "oops")) }),
                )],
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn step_keyed_records_content_hash() {
        let mut ctx = CruxCtx::new("test_agent");
        let val: i32 = ctx
            .step_keyed("fetch", &"url_a", || async { Ok(42) })
            .await
            .unwrap();
        assert_eq!(val, 42);
        assert_eq!(ctx.snapshot_steps().len(), 1);
        assert!(ctx.snapshot_steps()[0].content_hash.is_some());
    }

    #[tokio::test]
    async fn step_keyed_replays_with_content_hash() {
        // Run once to get a trace.
        let mut ctx1 = CruxCtx::new("test_agent");
        let _: String = ctx1
            .step_keyed("fetch", &"input_a", || async { Ok("result_a".to_string()) })
            .await
            .unwrap();
        let trace = ctx1.finalize::<serde_json::Value>(Ok(serde_json::json!(null)));

        // Replay with the same content key — should hit.
        let mut ctx2 = CruxCtx::new("test_agent");
        ctx2.replay.set_mode(crate::replay::ReplayMode::Lenient);
        ctx2.replay.seed_from(&trace);
        let val: String = ctx2
            .step_keyed("fetch", &"input_a", || async {
                panic!("should not execute — replayed");
            })
            .await
            .unwrap();
        assert_eq!(val, "result_a");
        assert_eq!(ctx2.snapshot_steps()[0].attempt, 0); // replayed
    }
}

#[cfg(test)]
mod proptest_confidence_range {
    use super::ConfidenceRange;
    use proptest::prelude::*;

    // Generate valid (lo, hi) pairs: both in [0.0, 1.0], lo < hi, both finite.
    fn valid_pair() -> impl Strategy<Value = (f32, f32)> {
        (0.0f32..1.0f32).prop_flat_map(|lo| {
            let hi_range = (lo + f32::EPSILON)..=1.0f32;
            (Just(lo), hi_range)
        })
    }

    proptest! {
        #[test]
        fn exclusive_accepts_valid_ranges((lo, hi) in valid_pair()) {
            let range = ConfidenceRange::exclusive(lo, hi);
            // A value at lo should be contained.
            prop_assert!(range.contains(lo));
            // A value exactly at hi should NOT be contained (exclusive upper bound).
            prop_assert!(!range.contains(hi));
        }

        #[test]
        fn inclusive_accepts_valid_ranges((lo, hi) in valid_pair()) {
            let range = ConfidenceRange::inclusive(lo, hi);
            // A value at lo should be contained.
            prop_assert!(range.contains(lo));
            // A value exactly at hi SHOULD be contained (inclusive upper bound).
            prop_assert!(range.contains(hi));
        }

        #[test]
        fn exclusive_rejects_equal_bounds(v in 0.0f32..1.0f32) {
            let result = std::panic::catch_unwind(|| ConfidenceRange::exclusive(v, v));
            prop_assert!(result.is_err(), "exclusive with lo==hi should panic");
        }

        #[test]
        fn exclusive_rejects_lo_greater_than_hi((lo, hi) in valid_pair()) {
            // Swapping makes hi < lo — should panic.
            let result = std::panic::catch_unwind(|| ConfidenceRange::exclusive(hi, lo));
            prop_assert!(result.is_err(), "exclusive with lo>hi should panic");
        }

        #[test]
        fn contains_is_monotone_between_bounds((lo, hi) in valid_pair()) {
            let range = ConfidenceRange::exclusive(lo, hi);
            let mid = lo + (hi - lo) / 2.0;
            // Midpoint must be inside.
            prop_assert!(range.contains(mid));
        }

        #[test]
        fn out_of_bounds_values_not_contained((lo, hi) in valid_pair()) {
            let range = ConfidenceRange::exclusive(lo, hi);
            // Values strictly below lo.
            if lo > 0.0 {
                prop_assert!(!range.contains(lo - f32::EPSILON));
            }
            // Values at or above hi.
            prop_assert!(!range.contains(hi));
        }
    }
}

// ─── if-guard: StepOpts, eval_expr, step_with_opts ───────────────────────────

use std::collections::HashMap;

/// Options for conditional step execution.
#[derive(Debug, Default)]
pub struct StepOpts {
    /// An optional `${{ ... }}` expression; if it evaluates to `false` the step
    /// is skipped and `step_with_opts` returns `Ok(None)`.
    pub if_expr: Option<String>,
    /// Per-step variable overrides (reserved for future use).
    pub vars: HashMap<String, serde_json::Value>,
}

impl CruxCtx {
    /// Run a step only when `opts.if_expr` evaluates to `true` (or is absent).
    ///
    /// Returns `Ok(Some(output))` when the step ran, `Ok(None)` when skipped.
    pub fn step_with_opts<F>(
        &mut self,
        alias: &str,
        opts: StepOpts,
        f: F,
    ) -> miette::Result<Option<serde_json::Value>>
    where
        F: FnOnce(&crux_types::step::StepState) -> miette::Result<serde_json::Value>,
    {
        if let Some(ref expr) = opts.if_expr {
            let state = self.state.read().expect("StepState lock poisoned");
            if !eval_expr(expr, &state)? {
                return Ok(None);
            }
        }
        let state = self.state.read().expect("StepState lock poisoned");
        let output = f(&state)?;
        drop(state);
        self.propagate_output(alias, output.clone());
        Ok(Some(output))
    }
}

/// Evaluate a simple `${{ expr }}` expression against step state.
///
/// Returns `true` if the step should run, `false` if it should be skipped.
///
/// Supported forms:
/// - `"true"` / `"false"` — boolean literals
/// - `""` or strings without `${{ }}` — always `true` (unconditional)
/// - `${{ outputs['alias'].field }}` — resolved to a string; `"false"`, `"0"`,
///   or `""` map to `false`; anything else maps to `true`
fn eval_expr(expr: &str, state: &crux_types::step::StepState) -> miette::Result<bool> {
    use miette::WrapErr as _;

    let expr = expr.trim();

    if expr.is_empty() || !expr.contains("${{") {
        return Ok(expr != "false");
    }

    let start = expr
        .find("${{")
        .ok_or_else(|| miette::miette!("missing '${{{{' in if_expr: {}", expr))?;
    let end = expr[start..]
        .find("}}")
        .map(|i| start + i + 2)
        .ok_or_else(|| miette::miette!("unclosed '${{{{' in if_expr: {}", expr))?;
    let inner = expr[start + 3..end - 2].trim();

    let resolved = resolve_output_ref_for_guard(inner, state)
        .wrap_err_with(|| format!("evaluating if_expr: {expr}"))?;

    Ok(!matches!(resolved.as_str(), "false" | "0" | ""))
}

fn resolve_output_ref_for_guard(
    expr: &str,
    state: &crux_types::step::StepState,
) -> miette::Result<String> {
    let rest = expr
        .strip_prefix("outputs['")
        .ok_or_else(|| miette::miette!("unsupported guard expression: {}", expr))?;
    let (alias, rest) = rest
        .split_once("']")
        .ok_or_else(|| miette::miette!("malformed alias in guard: {}", expr))?;
    let field_path = rest.trim_start_matches('.');

    let alias_val = state
        .get(alias)
        .ok_or_else(|| miette::miette!("alias '{alias}' not in state"))?;

    let val = if field_path.is_empty() {
        alias_val.clone()
    } else {
        let mut cur = alias_val;
        for seg in field_path.split('.') {
            cur = cur
                .get(seg)
                .ok_or_else(|| miette::miette!("field '{seg}' not found in alias '{alias}'"))?;
        }
        cur.clone()
    };

    Ok(match &val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    })
}

// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod if_guard_tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn eval_expr_literal_true() {
        let state = Default::default();
        assert!(eval_expr("true", &state).unwrap());
    }

    #[test]
    fn eval_expr_literal_false() {
        let state = Default::default();
        assert!(!eval_expr("false", &state).unwrap());
    }

    #[test]
    fn eval_expr_no_tokens_is_true() {
        let state = Default::default();
        assert!(eval_expr("", &state).unwrap());
    }

    #[test]
    fn eval_expr_field_access_resolves_truthy_nonzero() {
        use std::collections::HashMap;
        let mut state = HashMap::new();
        state.insert("build".to_string(), serde_json::json!({"exit_code": 0}));
        let result = eval_expr("${{ outputs['build'].exit_code }}", &state).unwrap();
        // exit_code = 0 → "0" → false
        assert!(!result);
    }

    #[test]
    fn eval_expr_missing_alias_returns_err() {
        let state = Default::default();
        assert!(eval_expr("${{ outputs['missing'].field }}", &state).is_err());
    }

    #[test]
    fn step_with_opts_skips_when_if_expr_false() {
        let mut ctx = CruxCtx::new_for_test();
        ctx.propagate_output("prev", serde_json::json!({"ok": false}));

        let opts = StepOpts {
            if_expr: Some("${{ outputs['prev'].ok }}".to_string()),
            vars: Default::default(),
        };

        let mut ran = false;
        let result = ctx.step_with_opts("my-step", opts, |_state| {
            ran = true;
            Ok(serde_json::json!({"done": true}))
        });

        assert!(result.unwrap().is_none()); // skipped → None
        assert!(!ran);
    }

    #[test]
    fn step_with_opts_runs_when_if_expr_true() {
        let mut ctx = CruxCtx::new_for_test();
        ctx.propagate_output("prev", serde_json::json!({"ok": true}));

        let opts = StepOpts {
            if_expr: Some("${{ outputs['prev'].ok }}".to_string()),
            vars: Default::default(),
        };

        let mut ran = false;
        let result = ctx.step_with_opts("my-step", opts, |_state| {
            ran = true;
            Ok(serde_json::json!({"done": true}))
        });

        assert!(result.unwrap().is_some()); // ran → Some
        assert!(ran);
    }

    proptest! {
        #[test]
        fn eval_expr_no_token_string_always_true(s in "[a-zA-Z0-9_]{1,20}") {
            let state = Default::default();
            let result = eval_expr(&s, &state);
            if s == "false" {
                prop_assert_eq!(result.unwrap(), false);
            } else if s == "true" {
                prop_assert_eq!(result.unwrap(), true);
            } else {
                prop_assert_eq!(result.unwrap(), true);
            }
        }
    }
}

#[cfg(test)]
mod step_state_tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn propagate_then_read_output_round_trips() {
        let ctx = CruxCtx::new_for_test();
        ctx.propagate_output("build", serde_json::json!({"exit_code": 0}));
        let val = ctx.read_output("build").unwrap();
        assert_eq!(val["exit_code"], 0);
    }

    #[test]
    fn read_missing_alias_returns_none() {
        let ctx = CruxCtx::new_for_test();
        assert!(ctx.read_output("nonexistent").is_none());
    }

    #[test]
    fn propagate_overwrites_previous_value() {
        let ctx = CruxCtx::new_for_test();
        ctx.propagate_output("step", serde_json::json!({"v": 1}));
        ctx.propagate_output("step", serde_json::json!({"v": 2}));
        assert_eq!(ctx.read_output("step").unwrap()["v"], 2);
    }

    proptest! {
        #[test]
        fn propagate_sequence_never_corrupts_other_aliases(
            key_a in "[a-z]{3,8}",
            key_b in "[a-z]{3,8}",
        ) {
            prop_assume!(key_a != key_b);
            let ctx = CruxCtx::new_for_test();
            ctx.propagate_output(&key_a, serde_json::json!("original_a"));
            ctx.propagate_output(&key_b, serde_json::json!("original_b"));
            ctx.propagate_output(&key_b, serde_json::json!("updated_b"));
            prop_assert_eq!(ctx.read_output(&key_a).unwrap(), serde_json::json!("original_a"));
            prop_assert_eq!(ctx.read_output(&key_b).unwrap(), serde_json::json!("updated_b"));
        }
    }
}

#[cfg(test)]
mod final_phase_tests {
    use super::determine_final_phase;
    use crux_types::crux_value::{FinalPhase, StepRecord as PhaseStepRecord};
    use proptest::prelude::*;

    #[test]
    fn empty_steps_returns_succeeded() {
        assert_eq!(determine_final_phase(&[], false), FinalPhase::Succeeded);
    }

    #[test]
    fn all_continue_on_error_returns_succeeded() {
        let steps = vec![
            PhaseStepRecord {
                alias: "a".into(),
                phase: FinalPhase::Failed,
                continue_on_error: true,
            },
            PhaseStepRecord {
                alias: "b".into(),
                phase: FinalPhase::Errored,
                continue_on_error: true,
            },
        ];
        assert_eq!(determine_final_phase(&steps, false), FinalPhase::Succeeded);
    }

    #[test]
    fn single_errored_returns_errored() {
        let steps = vec![
            PhaseStepRecord {
                alias: "a".into(),
                phase: FinalPhase::Succeeded,
                continue_on_error: false,
            },
            PhaseStepRecord {
                alias: "b".into(),
                phase: FinalPhase::Errored,
                continue_on_error: false,
            },
        ];
        assert_eq!(determine_final_phase(&steps, false), FinalPhase::Errored);
    }

    #[test]
    fn failed_does_not_override_errored() {
        let steps = vec![
            PhaseStepRecord {
                alias: "a".into(),
                phase: FinalPhase::Errored,
                continue_on_error: false,
            },
            PhaseStepRecord {
                alias: "b".into(),
                phase: FinalPhase::Failed,
                continue_on_error: false,
            },
        ];
        assert_eq!(determine_final_phase(&steps, false), FinalPhase::Errored);
    }

    #[test]
    fn ignore_continue_on_error_false_includes_all_steps() {
        // ignore_continue_on_error = true means: include all steps regardless of flag
        let steps = vec![PhaseStepRecord {
            alias: "a".into(),
            phase: FinalPhase::Failed,
            continue_on_error: true,
        }];
        assert_eq!(determine_final_phase(&steps, true), FinalPhase::Failed);
    }

    prop_compose! {
        fn arb_phase()(v in 0u8..5) -> FinalPhase {
            match v {
                0 => FinalPhase::Succeeded,
                1 => FinalPhase::Skipped,
                2 => FinalPhase::Aborted,
                3 => FinalPhase::Failed,
                _ => FinalPhase::Errored,
            }
        }
    }

    proptest! {
        #[test]
        fn result_gte_max_non_continue_on_error_phase(
            phases in proptest::collection::vec(arb_phase(), 1..10),
        ) {
            let steps: Vec<PhaseStepRecord> = phases.iter().map(|p| PhaseStepRecord {
                alias: "s".into(),
                phase: *p,
                continue_on_error: false,
            }).collect();
            let result = determine_final_phase(&steps, false);
            let expected = phases.iter().copied().max().unwrap();
            prop_assert_eq!(result, expected);
        }

        #[test]
        fn ord_is_total_and_consistent(a in arb_phase(), b in arb_phase()) {
            // Reflexive
            prop_assert_eq!(a.cmp(&a), std::cmp::Ordering::Equal);
            // Antisymmetric
            if a != b {
                prop_assert_ne!(a.cmp(&b), b.cmp(&a));
            }
        }
    }
}
