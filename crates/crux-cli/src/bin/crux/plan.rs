//! Rule-based and optional LLM-backed pipeline generation commands.

#[cfg(feature = "baml")]
use crux_plugin::discovery::{PluginDiscovery, TomlFileDiscovery};
use crux_script::{PipelineOutputFormat, format_pipeline_output};

#[cfg(feature = "baml")]
use crate::registry::resolve_plugins_path;

/// Canonical `PlanRule` / `RulePlanner` definitions live in `crux-types`.
pub use crux_types::planner::{PlanRule, RulePlanner};

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
        .map(|word| {
            word.chars()
                .filter(char::is_ascii_alphanumeric)
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<String>>()
        .join("-");
    let name = if name.is_empty() {
        "generated-pipeline"
    } else {
        &name
    };
    let mut out = format!("pipeline: \"{name}\"\nsteps:\n");
    for step in steps {
        out.push_str(&format!("  - step: {step}\n    handler: {step}\n"));
    }
    out
}

/// Generates a pipeline with the selected planner and writes it in the requested format.
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
        _ => cmd_plan_rule(goal, output, output_type),
    }
}

fn cmd_plan_rule(goal: &str, output: Option<&str>, output_type: &super::OutputType) {
    let steps = rule_planner_steps(goal);
    let yaml = steps_to_yaml(goal, &steps);
    let formatted = format_output_or_exit(&yaml, goal, output_type);
    write_output_or_exit(output, &formatted);
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

    let formatted = format_output_or_exit(&yaml, goal, output_type);

    write_output_or_exit(output, &formatted);
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

fn format_output_or_exit(yaml: &str, goal: &str, output_type: &super::OutputType) -> String {
    let format = match output_type {
        super::OutputType::Yaml => PipelineOutputFormat::Yaml,
        super::OutputType::Json => PipelineOutputFormat::Json,
        super::OutputType::Pretty => PipelineOutputFormat::Pretty,
        super::OutputType::DryRun => PipelineOutputFormat::DryRun,
        super::OutputType::Handoff => PipelineOutputFormat::Handoff,
    };
    format_pipeline_output(yaml, goal, format).unwrap_or_else(|error| {
        eprintln!("failed to format generated pipeline: {error}");
        std::process::exit(1);
    })
}

fn write_output_or_exit(path: Option<&str>, output: &str) {
    if let Some(path) = path {
        if let Err(error) = std::fs::write(path, output) {
            eprintln!("failed to write generated pipeline to {path}: {error}");
            std::process::exit(1);
        }
        eprintln!("Pipeline written to {path}");
    } else {
        println!("{output}");
    }
}
