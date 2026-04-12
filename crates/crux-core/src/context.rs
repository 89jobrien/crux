/// The Context trait — port for the agent runtime context.
///
/// Dependency inversion: Agent::run depends on this trait, not on CruxCtx directly.
/// CruxCtx is the production adapter. Tests can substitute a mock or no-op context.
use std::future::Future;

use crate::types::budget::Budget;
use crate::types::error::CruxErr;
use crate::types::recovery::Recovery;
use crate::types::step::Step;

pub trait Context: Send {
    /// Execute a named step, recording it in the trace.
    fn step<F, Fut, T>(
        &mut self,
        name: &str,
        f: F,
    ) -> impl Future<Output = Result<T, CruxErr>> + Send
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, CruxErr>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send;

    /// Execute a named step with a content key for replay identity.
    ///
    /// The key is hashed and stored alongside the step, allowing lenient replay
    /// to distinguish steps that share a name but differ in actual input.
    fn step_keyed<F, Fut, T, K>(
        &mut self,
        name: &str,
        key: &K,
        f: F,
    ) -> impl Future<Output = Result<T, CruxErr>> + Send
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, CruxErr>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send,
        K: serde::Serialize + Sync;

    /// Execute a named step with an explicit confidence score.
    fn step_with_confidence<F, Fut, T>(
        &mut self,
        name: &str,
        confidence: f32,
        f: F,
    ) -> impl Future<Output = Result<T, CruxErr>> + Send
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<T, CruxErr>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send;

    /// Execute a step with automatic retry support.
    fn step_retryable<F, Fut, T>(
        &mut self,
        name: &str,
        confidence: f32,
        make_fut: F,
    ) -> impl Future<Output = Result<T, CruxErr>> + Send
    where
        F: FnMut() -> Fut + Send,
        Fut: Future<Output = Result<T, CruxErr>> + Send,
        T: serde::Serialize + serde::de::DeserializeOwned + Send;

    /// Register a low-confidence handler.
    fn on_low_confidence<F, Fut>(&mut self, threshold: f32, handler: F)
    where
        F: Fn(f32) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static;

    /// Register a step-failure handler.
    fn on_step_failure<F, Fut>(&mut self, handler: F)
    where
        F: Fn(CruxErr) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static;

    /// Register a budget-exceeded handler.
    fn on_budget_exceeded<F, Fut>(&mut self, handler: F)
    where
        F: Fn(Budget) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static;

    /// Set the maximum retries per step.
    fn set_max_retries(&mut self, n: u32);

    /// Set a custom budget.
    fn set_budget(&mut self, budget: Budget);

    /// Record budget consumption.
    fn consume_budget(&mut self, amount: u64);

    /// Get the current budget.
    fn budget(&self) -> &Budget;

    /// Get remaining budget units.
    fn remaining_budget(&self) -> u64;

    /// Current step count.
    fn step_count(&self) -> u32;

    /// View recorded steps.
    fn snapshot_steps(&self) -> &[Step];
}
