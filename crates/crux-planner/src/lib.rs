//! crux-planner — goal-to-pipeline generation for crux-script.
//!
//! Two paths:
//! - Path A (LLM): `LlmPlanner` delegates to `crux-baml::planner` (BAML-routed,
//!   prefers a local Ollama, falls back to hosted providers)
//! - Path B (deterministic): `DeterministicPlanner` — rule-based, zero-latency, zero-cost

pub mod deterministic;
pub mod evolution;
pub mod generator;
pub mod llm;
pub mod metrics;
pub mod rule_planner;

pub use deterministic::DeterministicPlanner;
pub use generator::{InMemoryGenerator, LlmPlannerGeneric, PipelineGenerator};
pub use llm::LlmPlanner;

/// Domain errors for the crux-planner crate.
#[derive(Debug, thiserror::Error)]
pub enum PlannerError {
    #[error("no matching rule found for goal: {0}")]
    NoRuleMatch(String),
    #[error("pipeline generation failed: {0}")]
    Generation(String),
}

// Re-export canonical types from crux-types for backward compatibility.
pub use crux_types::metrics::{Goal, Intent, RunMetrics};

pub use deterministic::PlannerConfig;
pub use rule_planner::{PlanRule, RulePlanner};
