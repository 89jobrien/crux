/// cruxx-core: domain types, traits, and runtime for the cruxx agentic DSL.
#[macro_use]
mod trace;

pub mod agent;
pub mod approval;
pub mod context;
pub mod ctx;
pub mod delegation;
pub mod hooks;
pub mod recorder;
pub mod registry;
pub mod replay;
pub mod safety;
pub mod speculation;
pub mod types;

pub mod prelude {
    pub use crate::agent::Agent;
    pub use crate::approval::{ApprovalDecision, ApprovalGate, ApprovalRequest, RiskLevel};
    pub use crate::context::Context;
    pub use crate::ctx::{BoxFut, ConfidenceRange, ConfidenceRoute, CruxCtx, JoinArm, PipeStage};
    pub use crate::recorder::hash_content;
    pub use crate::registry::{Task, TaskRegistry, TaskStatus};
    pub use crate::replay::ReplayMode;
    pub use crate::safety::{SafetyPolicy, SafetyViolation};
    pub use crate::types::budget::Budget;
    pub use crate::types::crux_value::Crux;
    pub use crate::types::error::CruxErr;
    pub use crate::types::evolution::EvolutionOutcome;
    pub use crate::types::harness::{HarnessDiff, HarnessProfile, ResourceHints};
    pub use crate::types::id::{CruxId, TaskId};
    pub use crate::types::recovery::Recovery;
    pub use crate::types::step::{Step, StepKind, StepStatus};
    pub use slashcrux::{ExecutionContext, Priority, StepState, Urgency};
}
