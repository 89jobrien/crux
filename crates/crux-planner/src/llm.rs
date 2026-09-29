//! LLM-based planner — delegates to `crux_baml::planner::generate_pipeline`.

use crux_runtime::prelude::CruxErr;

/// Generates crux-script pipeline YAML from a natural language goal using an LLM.
///
/// Routed through BAML, so it prefers a local Ollama and needs no API key when
/// one is reachable, falling back to the hosted providers otherwise.
///
/// # Example
///
/// ```no_run
/// # use crux_planner::LlmPlanner;
/// # #[tokio::main] async fn main() -> Result<(), crux_runtime::prelude::CruxErr> {
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

    /// Generate a `.crux` pipeline YAML from a natural language `goal`.
    ///
    /// Returns the raw YAML string on success.
    pub async fn plan(&self, goal: &str) -> Result<String, CruxErr> {
        crux_baml::planner::generate_pipeline(
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

    /// Integration test: uses whatever BAML client resolves, preferring a local Ollama.
    /// Run with: `cargo nextest run -p crux-planner -- llm_planner_generates`
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
