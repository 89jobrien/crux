pub mod comparison;
pub mod evolution_adapter;
pub mod improvement;
pub mod metrics;
pub mod policy;

// Re-export crux types that improvement consumers need.
pub use crux_types::budget::Budget;
pub use crux_types::crux_value::Crux;
pub use crux_types::error::CruxErr;
pub use crux_types::id::CruxId;
pub use crux_types::step::{Step, StepKind, StepStatus};

pub use crux_runtime::safety::{SafetyPolicy, SafetyViolation};
pub use crux_runtime::types::harness::{HarnessDiff, HarnessProfile, ResourceHints};

// Public API from this crate.
pub use comparison::{Comparison, Verdict, replay_compare};
pub use evolution_adapter::{EvolutionPlanner, RunMetrics, evolution_to_strategy_diff};
pub use improvement::{
    DelegationAction, DelegationRule, Improvement, ImprovementKind, PromptPatch, Strategy,
    StrategyDiff,
};
pub use metrics::TraceMetrics;
pub use policy::{DefaultStrategyPolicy, StrategyPolicy, StrategyViolation};
