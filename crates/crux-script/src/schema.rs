/// YAML schema types for pipeline definitions.
use indexmap::IndexMap;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineDef {
    pub pipeline: String,
    #[serde(default)]
    pub budget: Option<BudgetDef>,
    pub steps: Vec<StepDef>,
}

/// An arm or stage in a join_all or pipe — either a bare handler name string,
/// or a full step object with `step`, optional `handler`, and optional `args`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ArmDef {
    /// Bare string: the name is both the step label and the handler name.
    Name(String),
    /// Full step object: `step` is the label, `handler` overrides the name, `args` injected.
    Step {
        step: String,
        #[serde(default)]
        handler: Option<String>,
        #[serde(default)]
        args: Option<serde_json::Value>,
    },
}

impl ArmDef {
    /// The label used in traces (step name).
    pub fn label(&self) -> &str {
        match self {
            ArmDef::Name(n) => n,
            ArmDef::Step { step, .. } => step,
        }
    }

    /// The handler name to look up in the registry.
    pub fn handler_name(&self) -> &str {
        match self {
            ArmDef::Name(n) => n,
            ArmDef::Step { step, handler, .. } => handler.as_deref().unwrap_or(step),
        }
    }

    /// Optional static args to inject into the handler input.
    pub fn args(&self) -> Option<&serde_json::Value> {
        match self {
            ArmDef::Name(_) => None,
            ArmDef::Step { args, .. } => args.as_ref(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BudgetDef {
    pub tokens: Option<u64>,
    pub calls: Option<u64>,
    pub duration_ms: Option<u64>,
    pub cost_cents: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StepDef {
    Step(StepNode),
    Delegate(DelegateNode),
    Pipe(PipeNode),
    JoinAll(JoinAllNode),
    RouteOnConfidence(RouteNode),
    Speculate(SpeculateNode),
}

#[derive(Debug, Clone, Deserialize)]
pub struct StepNode {
    pub step: String,
    #[serde(default)]
    pub handler: Option<String>,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    /// Declarative post-execution assertions (#82). Evaluated against the handler's
    /// output value after the step runs; a mismatch fails the pipeline with a
    /// descriptive error even though the handler itself succeeded.
    #[serde(default)]
    pub expect: Option<ExpectDef>,
}

/// Declarative assertions checked against a step's output value after execution.
///
/// Expects the output to be a JSON object with `exit_code` (number), `stdout`
/// (string), and/or `stderr` (string) fields — the convention used by shell-style
/// handlers. Fields not present in the `expect:` block are not checked.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExpectDef {
    #[serde(default)]
    pub exit_code: Option<i64>,
    #[serde(default)]
    pub stdout_contains: Option<String>,
    #[serde(default)]
    pub stderr_contains: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DelegateNode {
    pub delegate: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub budget: Option<BudgetDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PipeNode {
    pub pipe: String,
    pub stages: Vec<ArmDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JoinAllNode {
    pub join_all: String,
    pub arms: Vec<ArmDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteNode {
    pub route_on_confidence: String,
    pub value: String,
    pub routes: Vec<RouteBranch>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteBranch {
    pub range: String,
    pub label: String,
    pub handler: String,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpeculateNode {
    pub speculate: String,
    #[serde(default = "default_speculate_mode")]
    pub mode: SpeculateMode,
    pub arms: Vec<ArmDef>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpeculateMode {
    #[default]
    PickBest,
    FirstOk,
}

fn default_speculate_mode() -> SpeculateMode {
    SpeculateMode::PickBest
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compound_budget_preserves_all_constraints() {
        let yaml = r#"
pipeline: test
budget:
  calls: 40
  duration_ms: 900000
steps:
  - step: s1
"#;
        let def: PipelineDef = serde_saphyr::from_str(yaml).expect("parse");
        let budget = def.budget.expect("budget should be present");
        assert!(budget.calls.is_some(), "calls should be present");
        assert_eq!(budget.calls.unwrap(), 40);
        assert!(
            budget.duration_ms.is_some(),
            "duration_ms should be present"
        );
        assert_eq!(budget.duration_ms.unwrap(), 900_000);
    }

    #[test]
    fn single_budget_field_works() {
        let yaml = r#"
pipeline: test
budget:
  tokens: 5000
steps:
  - step: s1
"#;
        let def: PipelineDef = serde_saphyr::from_str(yaml).expect("parse");
        let budget = def.budget.expect("budget should be present");
        assert_eq!(budget.tokens.unwrap(), 5000);
        assert!(budget.calls.is_none());
        assert!(budget.duration_ms.is_none());
        assert!(budget.cost_cents.is_none());
    }
}

// ---------------------------------------------------------------------------
// Cruxfile (multi-target) schema
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct CruxfileDef {
    pub project: String,
    pub default: String,
    #[serde(default)]
    pub budget: Option<BudgetDef>,
    pub targets: IndexMap<String, TargetDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetDef {
    #[serde(default)]
    pub depends: Vec<String>,
    #[serde(default)]
    pub budget: Option<BudgetDef>,
    #[serde(default)]
    pub steps: Vec<StepDef>,
}
