//! Abstract step intents produced by a Planner.
use serde::{Deserialize, Serialize};

/// The name and scheduling priority of a step the planner permits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepIntent {
    /// Step name, matches the name passed to `ctx.step()`.
    pub name: String,
    /// Advisory scheduling priority (0 = normal, higher = prefer earlier).
    pub priority: u8,
}

/// Abstract action the planner emits for each step request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Execute the step normally.
    Execute(StepIntent),
    /// Skip the step (record as Skipped).
    Skip { name: String },
    /// Finish the agent run immediately (budget exhausted or policy stop).
    Finish { reason: String },
}

impl Action {
    pub fn name(&self) -> &str {
        match self {
            Action::Execute(i) => &i.name,
            Action::Skip { name } => name,
            Action::Finish { .. } => "<finish>",
        }
    }
}
