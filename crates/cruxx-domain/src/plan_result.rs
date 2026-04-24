//! Verdict returned by a Planner for a step request.
use serde::{Deserialize, Serialize};
use crate::action::Action;

/// What the planner decided to do with a requested step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum PlanResult {
    /// Execute as requested. Contains the (possibly rewritten) action.
    Allow(Action),
    /// Block the step. Agent receives a `CruxErr::Denied` error.
    Deny { reason: String },
    /// Return a synthetic output without executing. Used for dry-run/simulation.
    Simulate { output: serde_json::Value },
}
