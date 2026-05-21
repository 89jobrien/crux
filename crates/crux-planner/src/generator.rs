//! Port: `PipelineGenerator` — the boundary between the planner domain and any
//! LLM or external pipeline-generation backend.
//!
//! Implementations live in adapters:
//! - `InMemoryGenerator`   — deterministic test double (always compiled)
//! - The real BAML adapter is wired in `llm.rs` (feature `baml`)

use crate::PlannerError;

// ── Port ─────────────────────────────────────────────────────────────────────

/// Synchronous pipeline generation port.
///
/// Implement this trait to provide any backend — LLM, rule-engine, template,
/// or test double — for use with [`LlmPlannerGeneric`].
pub trait PipelineGenerator {
    /// Generate a `.crux` pipeline YAML string from a natural language `goal`.
    fn generate(&self, goal: &str) -> Result<String, PlannerError>;
}

// ── In-memory test double ────────────────────────────────────────────────────

/// A canned-response `PipelineGenerator` for unit/snapshot tests.
///
/// Returns the same string for every call — no network, no API key required.
#[derive(Debug, Clone)]
pub struct InMemoryGenerator {
    response: String,
}

impl InMemoryGenerator {
    /// Create a generator that always returns `response`.
    pub fn new(response: String) -> Self {
        Self { response }
    }
}

impl PipelineGenerator for InMemoryGenerator {
    fn generate(&self, _goal: &str) -> Result<String, PlannerError> {
        Ok(self.response.clone())
    }
}

// ── Generic LLM planner ──────────────────────────────────────────────────────

/// A planner that delegates pipeline generation to a [`PipelineGenerator`].
///
/// In production use the BAML-backed adapter (feature `baml`).
/// In tests use [`InMemoryGenerator`].
///
/// # Example
///
/// ```
/// use crux_planner::generator::{InMemoryGenerator, LlmPlannerGeneric};
///
/// let stub = InMemoryGenerator::new("pipeline: test\nsteps: []\n".into());
/// let planner = LlmPlannerGeneric::new(stub);
/// let yaml = planner.plan("do something").unwrap();
/// assert!(yaml.contains("pipeline:"));
/// ```
#[derive(Debug, Clone)]
pub struct LlmPlannerGeneric<G: PipelineGenerator> {
    generator: G,
}

impl<G: PipelineGenerator> LlmPlannerGeneric<G> {
    /// Create a new planner backed by `generator`.
    pub fn new(generator: G) -> Self {
        Self { generator }
    }

    /// Generate a `.crux` pipeline YAML from a natural language `goal`.
    pub fn plan(&self, goal: &str) -> Result<String, PlannerError> {
        self.generator.generate(goal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_generator_returns_canned_response() {
        let stub = InMemoryGenerator::new("pipeline: test\nsteps: []\n".into());
        let result = stub.generate("any goal").unwrap();
        assert_eq!(result, "pipeline: test\nsteps: []\n");
    }

    #[test]
    fn llm_planner_generic_delegates_to_generator() {
        let stub = InMemoryGenerator::new("pipeline: foo\nsteps:\n  - step: bar\n".into());
        let planner = LlmPlannerGeneric::new(stub);
        let yaml = planner.plan("some goal").unwrap();
        assert!(yaml.contains("pipeline: foo"));
        assert!(yaml.contains("step: bar"));
    }
}
