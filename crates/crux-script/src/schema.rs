/// YAML schema types for pipeline definitions.
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineDef {
    pub pipeline: String,
    #[serde(default)]
    pub budget: Option<BudgetDef>,
    pub steps: Vec<StepDef>,
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
    pub stages: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JoinAllNode {
    pub join_all: String,
    pub arms: Vec<String>,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpeculateNode {
    pub speculate: String,
    #[serde(default = "default_speculate_mode")]
    pub mode: SpeculateMode,
    pub arms: Vec<String>,
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
