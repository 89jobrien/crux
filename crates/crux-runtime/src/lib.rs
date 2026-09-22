//! crux-runtime: domain types, traits, and runtime for the crux agentic DSL.
//!
//! Core runtime providing `CruxCtx`, `Agent` trait, `TaskRegistry`,
//! replay, hooks, delegation, speculation, and governance primitives.
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
pub mod observability;
pub mod planner_gate;
pub mod recorder;
pub mod registry;
pub mod replay;
pub mod safety;
pub mod speculation;
pub mod trust;
pub mod types;

#[cfg(kani)]
mod kani_proofs;

pub mod prelude {
    pub use crux_domain::action::{Action, StepIntent};
    pub use crux_domain::plan_result::PlanResult;
    pub use crux_domain::planner::{DenyAllPlanner, PassthroughPlanner, Planner, SimulatePlanner};

    pub use crate::agent::{Agent, infer_priority};
    pub use crate::approval::{ApprovalDecision, ApprovalGate, ApprovalRequest, RiskLevel};
    pub use crate::audit::{AuditEntry, AuditSink, InMemoryAudit};
    pub use crate::context::{BudgetedInvocation, Context, InvocationMeter};
    pub use crate::ctx::{
        BoxFut, ConfidenceRange, ConfidenceRoute, CruxCtx, JoinArm, PipeFailurePolicy, PipeStage,
        RecoverablePipeStage,
    };
    pub use crate::governance::{GovernancePolicy, PolicyAction, compose_policies};
    #[cfg(feature = "tracing")]
    pub use crate::observability::emit_trace_spans;
    pub use crate::observability::trace_to_jsonl;
    pub use crate::recorder::hash_content;
    pub use crate::registry::{Task, TaskQuery, TaskRegistry, TaskStatus};
    pub use crate::replay::ReplayMode;
    pub use crate::safety::{SafetyPolicy, SafetyViolation};
    pub use crate::speculation::{BranchScore, ScoreWeights};
    pub use crate::trust::{TrustRegistry, TrustScore};
    pub use crate::types::budget::{
        Budget, BudgetLedger, BudgetLedgerEntry, BudgetUsage, HandlerUsage, UsdAmount,
    };
    pub use crate::types::crux_value::Crux;
    pub use crate::types::error::CruxErr;
    pub use crate::types::evolution::EvolutionOutcome;
    pub use crate::types::harness::{HarnessDiff, HarnessProfile, ResourceHints};
    pub use crate::types::id::{CruxId, TaskId};
    pub use crate::types::recovery::{Recovery, RecoveryChain};
    pub use crate::types::step::{Step, StepKind, StepStatus};
    pub use slashcrux::{ExecutionContext, Priority, StepState, Urgency};
}
