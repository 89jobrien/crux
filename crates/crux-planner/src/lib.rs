//! crux-planner — goal-to-pipeline generation for crux-script.
//!
//! Path B stub: deterministic planner types for future implementation.
//! The LLM-based planner (Path A) lives in `crux-agentic::planner`.

use serde::{Deserialize, Serialize};

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
