/// YAML schema types for pipeline definitions.
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
#[serde(untagged)]
pub enum BudgetDef {
    Tokens { tokens: u64 },
    Calls { calls: u64 },
    Duration { duration_ms: u64 },
    CostCents { cost_cents: u64 },
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
