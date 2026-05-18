//! Planner port — decides what to do with each step request.
//!
//! The Planner trait is sync and stateless. Implementations can gate steps,
//! rewrite priorities, simulate outputs, or stop execution entirely.
//! CruxCtx calls `next_action` before executing each step.
use crate::action::{Action, StepIntent};
use crate::plan_result::PlanResult;

/// Port: decides the fate of each step before execution.
///
/// - Return `Allow` to execute normally (optionally with rewritten priority).
/// - Return `Deny` to fail the step with a policy error.
/// - Return `Simulate` to return a synthetic output without executing.
pub trait Planner: Send + Sync + 'static {
    fn next_action(&self, step_name: &str, priority: u8) -> PlanResult;
}

/// Default planner: allows all steps through with unchanged priority.
///
/// Used when no custom planner is attached to a `CruxCtx`.
pub struct PassthroughPlanner;

impl Planner for PassthroughPlanner {
    fn next_action(&self, step_name: &str, priority: u8) -> PlanResult {
        PlanResult::Allow(Action::Execute(StepIntent {
            name: step_name.to_string(),
            priority,
        }))
    }
}

/// Planner that denies all steps — useful as a dry-run sentinel in tests.
pub struct DenyAllPlanner {
    pub reason: String,
}

impl Planner for DenyAllPlanner {
    fn next_action(&self, _name: &str, _priority: u8) -> PlanResult {
        PlanResult::Deny {
            reason: self.reason.clone(),
        }
    }
}

/// Planner that simulates all steps with a fixed output value.
pub struct SimulatePlanner {
    pub output: serde_json::Value,
}

impl Planner for SimulatePlanner {
    fn next_action(&self, _name: &str, _priority: u8) -> PlanResult {
        PlanResult::Simulate {
            output: self.output.clone(),
        }
    }
}
