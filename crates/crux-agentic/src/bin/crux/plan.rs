#[cfg(feature = "baml")]
use crux_plugin::discovery::{PluginDiscovery, TomlFileDiscovery};
#[cfg(feature = "baml")]
use crux_script::schema::PipelineDef;
#[cfg(feature = "baml")]
use serde_json::{Value, json};

// TODO(review): add `use super::OutputType;` to avoid repeated `super::OutputType` refs
#[cfg(feature = "baml")]
use crate::registry::{collect_handler_names, resolve_plugins_path};

/// A rule mapping goal keywords to a handler step sequence.
pub struct PlanRule {
    pub keywords: Vec<String>,
    pub steps: Vec<String>,
}

impl PlanRule {
    pub fn new(
        keywords: impl IntoIterator<Item = impl Into<String>>,
        steps: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            keywords: keywords.into_iter().map(Into::into).collect(),
            steps: steps.into_iter().map(Into::into).collect(),
        }
    }

    fn matches(&self, goal_lower: &str) -> bool {
        self.keywords
            .iter()
            .all(|kw| goal_lower.contains(kw.to_lowercase().as_str()))
    }
}

/// Deterministic rule-based planner (no network, no API key).
pub struct RulePlanner {
    rules: Vec<PlanRule>,
    default_steps: Vec<String>,
}

impl RulePlanner {
    pub fn new(rules: Vec<PlanRule>, default_steps: Vec<String>) -> Self {
        Self {
            rules,
            default_steps,
        }
    }

    pub fn plan(&self, goal: &str) -> Vec<String> {
        let lower = goal.to_lowercase();
        self.rules
            .iter()
            .find(|r| r.matches(&lower))
            .map(|r| r.steps.clone())
            .unwrap_or_else(|| self.default_steps.clone())
    }
}

/// Build the default RulePlanner and return the step sequence for `goal`.
pub fn rule_planner_steps(goal: &str) -> Vec<String> {
    let rules = default_plan_rules();
    let planner = RulePlanner::new(rules, vec!["shell::capture".into()]);
    planner.plan(goal)
}

/// Default rule set used by the `plan --planner rule` subcommand.
pub fn default_plan_rules() -> Vec<PlanRule> {
    vec![
        PlanRule::new(["fetch", "summarize"], ["http::get", "llm::complete"]),
        PlanRule::new(["fetch"], ["http::get"]),
        PlanRule::new(["summarize"], ["llm::complete"]),
    ]
}

/// Serialize a step list to a minimal YAML pipeline string.
fn steps_to_yaml(goal: &str, steps: &[String]) -> String {
    let name = goal
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join("-");
    let mut out = format!("pipeline: {name}\nsteps:\n");
    for step in steps {
        out.push_str(&format!("  - step: {step}\n    handler: {step}\n"));
    }
    out
}

pub fn cmd_plan(
    goal: &str,
    output: Option<&str>,
    constraints: Option<&str>,
    output_type: &super::OutputType,
    plugins_path: Option<&str>,
    planner: &str,
) {
    match planner {
        "llm" => cmd_plan_llm(goal, output, constraints, output_type, plugins_path),
        _ => cmd_plan_rule(goal, output),
    }
}

fn cmd_plan_rule(goal: &str, output: Option<&str>) {
    let steps = rule_planner_steps(goal);
    let yaml = steps_to_yaml(goal, &steps);
    if let Some(path) = output {
        std::fs::write(path, &yaml).expect("failed to write output file");
        eprintln!("Pipeline written to {path}");
    } else {
        println!("{yaml}");
    }
}

#[cfg(feature = "baml")]
fn cmd_plan_llm(
    goal: &str,
    output: Option<&str>,
    constraints: Option<&str>,
    output_type: &super::OutputType,
    plugins_path: Option<&str>,
) {
    let has_openai = std::env::var("OPENAI_API_KEY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let has_anthropic = std::env::var("ANTHROPIC_API_KEY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if !has_openai && !has_anthropic {
        eprintln!(
            "[crux] warning: `plan` requires an LLM API key but neither \
             OPENAI_API_KEY nor ANTHROPIC_API_KEY is set"
        );
        eprintln!(
            "[crux] hint: copy .env.example to .env and configure, \
             or use `dotenvx run -- crux plan ...`"
        );
    }

    let rt = tokio::runtime::Runtime::new().unwrap();

    let disc = TomlFileDiscovery::new(resolve_plugins_path(plugins_path));
    let entries = disc.discover().unwrap_or_default();
    let extra: Vec<String> = entries
        .iter()
        .map(|p| format!("{}::* -- plugin (see plugins.toml)", p.name))
        .collect();

    let yaml = rt
        .block_on(crux_baml::planner::generate_pipeline(
            goal,
            constraints,
            &extra,
        ))
        .expect("pipeline generation failed");

    let formatted = format_plan_output(&yaml, goal, output_type);

    if let Some(path) = output {
        std::fs::write(path, &formatted).expect("failed to write output file");
        eprintln!("Pipeline written to {path}");
    } else {
        println!("{formatted}");
    }
}

#[cfg(not(feature = "baml"))]
fn cmd_plan_llm(
    _goal: &str,
    _output: Option<&str>,
    _constraints: Option<&str>,
    _output_type: &super::OutputType,
    _plugins_path: Option<&str>,
) {
    eprintln!(
        "crux plan --planner llm requires --features baml. \
         Run: cargo build --features baml"
    );
    std::process::exit(1);
}

#[cfg(feature = "baml")]
fn format_plan_output(yaml: &str, goal: &str, output_type: &super::OutputType) -> String {
    match output_type {
        super::OutputType::Yaml => yaml.to_string(),

        super::OutputType::Json => {
            let pipeline: PipelineDef = crux_script::load(yaml).expect("generated YAML is invalid");
            let steps: Vec<Value> = collect_handler_names(&pipeline)
                .into_iter()
                .map(|h| json!({ "handler": h }))
                .collect();
            serde_json::to_string_pretty(&json!({
                "pipeline": pipeline.pipeline,
                "steps": steps,
                "yaml": yaml,
            }))
            .unwrap()
        }

        super::OutputType::Pretty => {
            let pipeline: PipelineDef = crux_script::load(yaml).expect("generated YAML is invalid");
            let handlers = collect_handler_names(&pipeline);
            let mut out = String::new();
            out.push_str(&format!("# Generated pipeline: {}\n", pipeline.pipeline));
            out.push_str(&format!("# Goal: {goal}\n"));
            out.push_str(&format!("# Steps: {}\n", pipeline.steps.len()));
            out.push_str(&format!("# Handlers: {}\n", handlers.join(", ")));
            out.push_str("#\n\n");
            out.push_str(yaml);
            out
        }

        super::OutputType::DryRun => {
            let pipeline: PipelineDef = crux_script::load(yaml).expect("generated YAML is invalid");
            let handlers = collect_handler_names(&pipeline);
            let mut out = String::new();
            out.push_str(&format!(
                "Pipeline: {} ({} steps)\n\n",
                pipeline.pipeline,
                pipeline.steps.len()
            ));
            for (i, name) in handlers.iter().enumerate() {
                out.push_str(&format!("  {:>2}. {name}\n", i + 1));
            }
            out
        }

        super::OutputType::Handoff => {
            let pipeline: PipelineDef = crux_script::load(yaml).expect("generated YAML is invalid");
            format_handoff(&pipeline, goal)
        }
    }
}

#[cfg(feature = "baml")]
fn format_handoff(pipeline: &PipelineDef, goal: &str) -> String {
    use crux_script::schema::StepDef;

    let mut out = String::new();
    out.push_str(&format!("project: {}\n", pipeline.pipeline));
    out.push_str(&format!("id: {}\n", pipeline.pipeline));
    out.push_str(&format!(
        "description: >\n  Generated from goal: {goal}\n\n"
    ));
    out.push_str("items:\n\n");

    for (i, step) in pipeline.steps.iter().enumerate() {
        let (id, name, handler) = match step {
            StepDef::Step(n) => (&n.step, &n.step, n.handler.as_deref().unwrap_or(&n.step)),
            StepDef::Pipe(n) => (&n.pipe, &n.pipe, n.pipe.as_str()),
            StepDef::JoinAll(n) => (&n.join_all, &n.join_all, n.join_all.as_str()),
            StepDef::Delegate(n) => (&n.delegate, &n.delegate, n.delegate.as_str()),
            StepDef::Speculate(n) => (&n.speculate, &n.speculate, n.speculate.as_str()),
            StepDef::RouteOnConfidence(n) => (
                &n.route_on_confidence,
                &n.route_on_confidence,
                n.route_on_confidence.as_str(),
            ),
        };

        out.push_str(&format!("  - id: step-{}\n", i + 1));
        out.push_str(&format!("    name: {name}\n"));
        out.push_str(&format!("    title: \"Execute {id} via {handler}\"\n"));
        out.push_str(&format!(
            "    description: >\n      Pipeline step {}: {handler}\n",
            i + 1
        ));
        out.push_str("    priority: P1\n");
        out.push_str("    status: open\n\n");
    }

    out
}
