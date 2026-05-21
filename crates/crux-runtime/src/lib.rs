/// crux-runtime: domain types, traits, and runtime for the crux agentic DSL.
// TODO(#78): EDDOS-style event aggregation — unify heterogeneous step types into a
//   typed event stream (MPSC -> enrichment -> batching -> broadcast) for analytics,
//   replay filtering, and multi-agent coordination (cf. devloop)
#[macro_use]
mod trace;

pub mod agent;
pub mod approval;
pub mod audit;
pub mod context;
pub mod ctx;
pub mod delegation;
pub mod event_sink;
pub mod governance;
pub mod hooks;
pub mod planner_gate;
pub mod recorder;
pub mod registry;
pub mod replay;
pub mod safety;
pub mod speculation;
pub mod trust;
pub mod types;

pub mod prelude {
    pub use crux_domain::action::{Action, StepIntent};
    pub use crux_domain::plan_result::PlanResult;
    pub use crux_domain::planner::{DenyAllPlanner, PassthroughPlanner, Planner, SimulatePlanner};

    pub use crate::agent::Agent;
    pub use crate::approval::{ApprovalDecision, ApprovalGate, ApprovalRequest, RiskLevel};
    pub use crate::audit::{AuditEntry, AuditSink, InMemoryAudit};
    pub use crate::context::Context;
    pub use crate::ctx::{BoxFut, ConfidenceRange, ConfidenceRoute, CruxCtx, JoinArm, PipeStage};
    pub use crate::governance::{GovernancePolicy, PolicyAction, compose_policies};
    pub use crate::recorder::hash_content;
    pub use crate::registry::{Task, TaskRegistry, TaskStatus};
    pub use crate::replay::ReplayMode;
    pub use crate::safety::{SafetyPolicy, SafetyViolation};
    pub use crate::trust::{TrustRegistry, TrustScore};
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
