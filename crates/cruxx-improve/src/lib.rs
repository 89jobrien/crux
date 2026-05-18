pub mod comparison;
pub mod evolution_adapter;
pub mod improvement;
pub mod metrics;
pub mod policy;

// Re-export crux types that improvement consumers need.
pub use cruxx_types::budget::Budget;
pub use cruxx_types::crux_value::Crux;
pub use cruxx_types::error::CruxErr;
pub use cruxx_types::id::CruxId;
pub use cruxx_types::step::{Step, StepKind, StepStatus};

pub use cruxx_core::safety::{SafetyPolicy, SafetyViolation};
pub use cruxx_core::types::harness::{HarnessDiff, HarnessProfile, ResourceHints};

// Public API from this crate.
pub use comparison::{Comparison, Verdict, replay_compare};
pub use evolution_adapter::{EvolutionPlanner, RunMetrics, evolution_to_strategy_diff};
pub use improvement::{
    DelegationAction, DelegationRule, Improvement, ImprovementKind, PromptPatch, Strategy,
    StrategyDiff,
};
pub use metrics::TraceMetrics;
pub use policy::{DefaultStrategyPolicy, StrategyPolicy, StrategyViolation};
