//! crux-planner — goal-to-pipeline generation for crux-script.
//!
//! Two paths:
//! - Path A (LLM): `LlmPlanner` delegates to `crux-agentic::planner` (feature `baml`)
//! - Path B (deterministic): `DeterministicPlanner` — rule-based, zero-latency, zero-cost

use serde::{Deserialize, Serialize};

pub mod deterministic;
pub mod evolution;
pub mod generator;
pub mod metrics;
pub mod rule_planner;

#[cfg(feature = "baml")]
pub mod llm;

#[cfg(feature = "baml")]
pub use llm::LlmPlanner;

pub use deterministic::DeterministicPlanner;
pub use generator::{InMemoryGenerator, LlmPlannerGeneric, PipelineGenerator};

/// Domain errors for the crux-planner crate.
#[derive(Debug, thiserror::Error)]
pub enum PlannerError {
    #[error("no matching rule found for goal: {0}")]
    NoRuleMatch(String),
    #[error("pipeline generation failed: {0}")]
    Generation(String),
}

/// A user-facing goal to be translated into a pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub description: String,
    pub constraints: Vec<String>,
}

/// Parsed intent extracted from a goal — intermediate representation
/// between natural language and a concrete pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub goal: String,
    pub input_source: Option<String>,
    pub output_destination: Option<String>,
    pub constraints: serde_json::Value,
    pub preferences: serde_json::Value,
}

pub use deterministic::PlannerConfig;
pub use rule_planner::{PlanRule, RulePlanner};
