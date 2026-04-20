//! cruxx-planner — goal-to-pipeline generation for cruxx-script.
//!
//! Two paths:
//! - Path A (LLM): lives in `cruxx-agentic::planner`
//! - Path B (deterministic): `EvolutionPlanner` for metrics-driven profile changes

use serde::{Deserialize, Serialize};

pub mod evolution;
pub mod metrics;

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

/// Configuration for deterministic pipeline generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerConfig {
    pub max_steps: usize,
    pub allowed_handlers: Vec<String>,
}
