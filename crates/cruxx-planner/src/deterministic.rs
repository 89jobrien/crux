//! Path B: rule-based deterministic pipeline composer.
//!
//! Maps goal keywords/patterns to handler sequences without any LLM call.
//! Zero latency, zero cost, fully deterministic: same input always produces
//! the same `.cruxx` pipeline YAML.

use crate::PlannerError;

/// Configuration for the deterministic planner.
#[derive(Debug, Clone)]
pub struct PlannerConfig {
    /// Pipeline name prefix inserted into generated YAML.
    pub pipeline_name: String,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            pipeline_name: "generated".to_string(),
        }
    }
}

/// A single rule: if the goal (lowercased) contains `keyword`, emit `handlers`
/// in order.
struct Rule {
    keyword: &'static str,
    handlers: &'static [&'static str],
}

/// Built-in composition rules, evaluated in declaration order.
/// The first matching rule wins; if none match the fallback rule is applied.
const RULES: &[Rule] = &[
    Rule {
        keyword: "git",
        handlers: &["git::diff", "llm::extract", "json::write"],
    },
    Rule {
        keyword: "extract",
        handlers: &["fs::read", "llm::extract", "json::write"],
    },
    Rule {
        keyword: "summarize",
        handlers: &["fs::read", "llm::extract", "json::write"],
    },
    Rule {
        keyword: "read",
        handlers: &["fs::read", "llm::extract"],
    },
    Rule {
        keyword: "write",
        handlers: &["fs::read", "llm::extract", "json::write"],
    },
    Rule {
        keyword: "json",
        handlers: &["fs::read", "json::parse", "json::write"],
    },
];

/// Fallback handler sequence used when no rule keyword matches.
const FALLBACK: &[&str] = &["shell::capture", "json::write"];

/// Deterministic, rule-based pipeline composer.
///
/// # Example
///
/// ```
/// use cruxx_planner::deterministic::{DeterministicPlanner, PlannerConfig};
///
/// let planner = DeterministicPlanner::new(PlannerConfig::default());
/// let yaml = planner.plan("Read a file and extract entities").unwrap();
/// assert!(yaml.contains("fs::read"));
/// assert!(yaml.contains("pipeline:"));
/// ```
#[derive(Debug, Clone)]
pub struct DeterministicPlanner {
    config: PlannerConfig,
}

impl DeterministicPlanner {
    /// Create a new planner with the given configuration.
    pub fn new(config: PlannerConfig) -> Self {
        Self { config }
    }

    /// Generate a `.cruxx` pipeline YAML from a natural language `goal`.
    ///
    /// Scans the goal (case-insensitively) against built-in keyword rules.
    /// The first matching rule determines the handler sequence.  If no rule
    /// matches, a `shell::capture` fallback pipeline is returned.
    pub fn plan(&self, goal: &str) -> Result<String, PlannerError> {
        let lower = goal.to_lowercase();
        let handlers = RULES
            .iter()
            .find(|r| lower.contains(r.keyword))
            .map(|r| r.handlers)
            .unwrap_or(FALLBACK);

        Ok(self.render(goal, handlers))
    }

    // ── private ──────────────────────────────────────────────────────────────

    fn render(&self, goal: &str, handlers: &[&str]) -> String {
        let mut out = String::new();
        out.push_str(&format!("pipeline: {}\n", self.config.pipeline_name));
        out.push_str(&format!("# goal: {goal}\n"));
        out.push_str("steps:\n");
        for (i, handler) in handlers.iter().enumerate() {
            let label = step_label(handler, i);
            out.push_str(&format!("  - step: {label}\n"));
            out.push_str(&format!("    handler: {handler}\n"));
        }
        out
    }
}

/// Derive a human-readable step label from a handler name and its index.
///
/// `"fs::read"` → `"read"`, `"llm::extract"` → `"extract"`, etc.
/// Falls back to `"step_{index}"` for unrecognised patterns.
fn step_label(handler: &str, index: usize) -> String {
    handler
        .split("::")
        .last()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("step_{index}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planner() -> DeterministicPlanner {
        DeterministicPlanner::new(PlannerConfig::default())
    }

    #[test]
    fn step_label_extracts_suffix() {
        assert_eq!(step_label("fs::read", 0), "read");
        assert_eq!(step_label("llm::extract", 1), "extract");
        assert_eq!(step_label("shell::capture", 0), "capture");
    }

    #[test]
    fn render_contains_pipeline_key() {
        let yaml = planner().render("test goal", &["fs::read"]);
        assert!(yaml.contains("pipeline:"));
        assert!(yaml.contains("steps:"));
        assert!(yaml.contains("fs::read"));
    }

    #[test]
    fn unknown_goal_uses_fallback() {
        let yaml = planner().plan("xyzzy frobnicate").unwrap();
        assert!(yaml.contains("shell::capture"));
    }
}
