//! LLM-based planner — delegates to `cruxx-agentic::planner::generate_pipeline`.

use cruxx_core::prelude::CruxErr;

/// Generates cruxx-script pipeline YAML from a natural language goal using an LLM.
///
/// Requires the `baml` feature and a valid `OPENAI_API_KEY` (or configured BAML client).
///
/// # Example
///
/// ```no_run
/// # use cruxx_planner::LlmPlanner;
/// # #[tokio::main] async fn main() -> Result<(), cruxx_core::prelude::CruxErr> {
/// let planner = LlmPlanner::new();
/// let yaml = planner.plan("Read a file and extract entities").await?;
/// println!("{yaml}");
/// # Ok(()) }
/// ```
#[derive(Debug, Default, Clone)]
pub struct LlmPlanner {
    /// Optional extra handler descriptions injected into the prompt.
    pub extra_handlers: Vec<String>,
    /// Optional free-form constraint text forwarded to the BAML prompt.
    pub constraints: Option<String>,
}

impl LlmPlanner {
    /// Create a new `LlmPlanner` with no extra handlers or constraints.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add extra handler descriptions (e.g. from a plugin registry).
    pub fn with_extra_handlers(mut self, handlers: Vec<String>) -> Self {
        self.extra_handlers = handlers;
        self
    }

    /// Set free-form constraint text forwarded to the BAML prompt.
    pub fn with_constraints(mut self, constraints: impl Into<String>) -> Self {
        self.constraints = Some(constraints.into());
        self
    }

    /// Generate a `.cruxx` pipeline YAML from a natural language `goal`.
    ///
    /// Returns the raw YAML string on success.
    pub async fn plan(&self, goal: &str) -> Result<String, CruxErr> {
        cruxx_agentic::planner::generate_pipeline(
            goal,
            self.constraints.as_deref(),
            &self.extra_handlers,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_planner_default_constructs() {
        let planner = LlmPlanner::new();
        assert!(planner.extra_handlers.is_empty());
        assert!(planner.constraints.is_none());
    }

    #[test]
    fn llm_planner_builder_methods() {
        let planner = LlmPlanner::new()
            .with_extra_handlers(vec!["custom::handler -- does something".into()])
            .with_constraints("budget: 1000 tokens");
        assert_eq!(planner.extra_handlers.len(), 1);
        assert!(planner.constraints.is_some());
    }

    /// Integration test: requires OPENAI_API_KEY (or configured BAML client).
    /// Run with: `cargo nextest run -p cruxx-planner --features baml -- llm_planner_generates`
    #[tokio::test]
    #[ignore = "requires live LLM credentials"]
    async fn llm_planner_generates_valid_yaml() {
        let planner = LlmPlanner::new();
        let yaml = planner
            .plan("Read a file and extract named entities")
            .await
            .expect("generate_pipeline failed");
        assert!(
            yaml.contains("pipeline:"),
            "expected 'pipeline:' key in output:\n{yaml}"
        );
        assert!(
            yaml.contains("steps:"),
            "expected 'steps:' key in output:\n{yaml}"
        );
    }
}
