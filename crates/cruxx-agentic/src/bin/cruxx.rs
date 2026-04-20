/// cruxx — pipeline runner and planner for cruxx-script.
///
/// Subcommands:
///   run   Execute a YAML pipeline
///   plan  Generate a pipeline from a natural language goal (requires `baml` feature)
use std::io::Read as _;
use std::sync::Arc;
use std::time::Instant;

use clap::{Parser, ValueEnum};
use cruxx_core::prelude::*;
use cruxx_plugin::bridge::register_plugins;
use cruxx_plugin::manifest::load_manifest;
use cruxx_script::{HandlerRegistry, Runner, schema::PipelineDef};
use serde_json::{Value, json};

#[derive(Debug, Clone, ValueEnum)]
enum OutputType {
    /// Raw pipeline YAML (default)
    Yaml,
    /// Pipeline definition as JSON
    Json,
    /// YAML with explanatory header comment
    Pretty,
    /// Parse and print step names/handlers without executing
    DryRun,
    /// HANDOFF-compatible task list
    Handoff,
}

#[derive(Parser)]
#[command(name = "cruxx", about = "cruxx pipeline runner and planner")]
enum Cli {
    /// Validate a .crux pipeline file without executing it
    Check {
        /// Pipeline file(s) to validate
        #[arg(required = true)]
        pipelines: Vec<String>,
    },
    /// Execute a .crux pipeline ("-" reads from stdin)
    Run {
        /// Pipeline file ("-" for stdin)
        pipeline: String,
        /// Optional input JSON file
        input: Option<String>,
        /// Path to plugins.toml (default: ~/.cruxx/plugins.toml)
        #[arg(long)]
        plugins: Option<String>,
    },
    /// Generate a pipeline from a natural language goal
    #[cfg(feature = "baml")]
    Plan {
        /// Natural language goal
        #[arg(long)]
        goal: String,
        /// Output file (stdout if omitted)
        #[arg(short, long)]
        output: Option<String>,
        /// Optional constraints
        #[arg(long)]
        constraints: Option<String>,
        /// Output format
        #[arg(long, value_enum, default_value_t = OutputType::Yaml)]
        output_type: OutputType,
        /// Path to plugins.toml (default: ~/.cruxx/plugins.toml)
        #[arg(long)]
        plugins: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli {
        Cli::Check { pipelines } => cmd_check(&pipelines),
        Cli::Run {
            pipeline,
            input,
            plugins,
        } => cmd_run(&pipeline, input.as_deref(), plugins.as_deref()),
        #[cfg(feature = "baml")]
        Cli::Plan {
            goal,
            output,
            constraints,
            output_type,
            plugins,
        } => cmd_plan(
            &goal,
            output.as_deref(),
            constraints.as_deref(),
            &output_type,
            plugins.as_deref(),
        ),
    }
}

fn cmd_check(paths: &[String]) {
    let mut failures = 0u32;

    for path in paths {
        match cruxx_script::load_file(path) {
            Ok(pipeline) => {
                let handlers = collect_handler_names(&pipeline);
                let step_count = pipeline.steps.len();
                println!(
                    "{path}: valid ({step_count} steps, handlers: {})",
                    handlers.join(", ")
                );
            }
            Err(e) => {
                eprintln!("{path}: {e}");
                failures += 1;
            }
        }
    }

    if failures > 0 {
        std::process::exit(1);
    }
}

fn cmd_run(pipeline_path: &str, input_path: Option<&str>, plugins_path: Option<&str>) {
    let input: Value = if let Some(path) = input_path {
        let contents = std::fs::read_to_string(path).expect("failed to read input file");
        serde_json::from_str(&contents).expect("invalid JSON input")
    } else {
        Value::Null
    };

    let pipeline = if pipeline_path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .expect("failed to read stdin");
        cruxx_script::load(&buf).expect("failed to parse pipeline from stdin")
    } else {
        cruxx_script::load_file(pipeline_path).expect("failed to load pipeline")
    };

    warn_missing_env(&pipeline);

    let rt = tokio::runtime::Runtime::new().unwrap();
    let registry = rt.block_on(build_registry(&pipeline, plugins_path));
    let runner = Runner::new(Arc::new(registry));

    let start = Instant::now();
    let cruxx = rt.block_on(runner.run(&pipeline, input));
    let elapsed = start.elapsed();

    print_trace(&cruxx, elapsed);
}

#[cfg(feature = "baml")]
fn cmd_plan(
    goal: &str,
    output: Option<&str>,
    constraints: Option<&str>,
    output_type: &OutputType,
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
            "[cruxx] warning: `plan` requires an LLM API key but neither \
             OPENAI_API_KEY nor ANTHROPIC_API_KEY is set"
        );
        eprintln!(
            "[cruxx] hint: copy .env.example to .env and configure, \
             or use `dotenvx run -- cruxx plan ...`"
        );
    }

    let rt = tokio::runtime::Runtime::new().unwrap();

    let manifest = load_manifest(resolve_plugins_path(plugins_path)).unwrap_or_default();
    let extra: Vec<String> = manifest
        .plugin
        .iter()
        .map(|p| format!("{}::* -- plugin (see plugins.toml)", p.name))
        .collect();

    let yaml = rt
        .block_on(cruxx_agentic::planner::generate_pipeline(
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

#[cfg(feature = "baml")]
fn format_plan_output(yaml: &str, goal: &str, output_type: &OutputType) -> String {
    match output_type {
        OutputType::Yaml => yaml.to_string(),

        OutputType::Json => {
            let pipeline: PipelineDef =
                cruxx_script::load(yaml).expect("generated YAML is invalid");
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

        OutputType::Pretty => {
            let pipeline: PipelineDef =
                cruxx_script::load(yaml).expect("generated YAML is invalid");
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

        OutputType::DryRun => {
            let pipeline: PipelineDef =
                cruxx_script::load(yaml).expect("generated YAML is invalid");
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

        OutputType::Handoff => {
            let pipeline: PipelineDef =
                cruxx_script::load(yaml).expect("generated YAML is invalid");
            format_handoff(&pipeline, goal)
        }
    }
}

#[cfg(feature = "baml")]
fn format_handoff(pipeline: &PipelineDef, goal: &str) -> String {
    use cruxx_script::schema::StepDef;

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

/// Warn if the pipeline uses LLM handlers but no API keys are set.
fn warn_missing_env(pipeline: &PipelineDef) {
    let handlers = collect_handler_names(pipeline);
    let needs_llm = handlers.iter().any(|h| h.starts_with("llm::"));
    if !needs_llm {
        return;
    }

    let has_openai = std::env::var("OPENAI_API_KEY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let has_anthropic = std::env::var("ANTHROPIC_API_KEY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);

    if !has_openai && !has_anthropic {
        eprintln!(
            "[cruxx] warning: pipeline uses llm:: handlers but neither \
             OPENAI_API_KEY nor ANTHROPIC_API_KEY is set"
        );
        eprintln!(
            "[cruxx] hint: copy .env.example to .env and configure, \
             or use `dotenvx run -- cruxx run ...`"
        );
    }
}

fn resolve_plugins_path(plugins_path: Option<&str>) -> String {
    plugins_path.map(String::from).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.cruxx/plugins.toml")
    })
}

/// Build a registry seeded with all cruxx-agentic built-in handlers.
async fn build_registry(pipeline: &PipelineDef, plugins_path: Option<&str>) -> HandlerRegistry {
    let manifest = load_manifest(resolve_plugins_path(plugins_path)).unwrap_or_default();

    let plugin_handler_descs: Vec<String> = manifest
        .plugin
        .iter()
        .map(|p| format!("{}::* -- plugin (see plugins.toml)", p.name))
        .collect();

    let mut reg = HandlerRegistry::new();
    cruxx_agentic::register_all_with_plugins(&mut reg, plugin_handler_descs);

    if !manifest.plugin.is_empty() {
        if let Err(e) = register_plugins(&mut reg, &manifest.plugin).await {
            eprintln!("[cruxx] warning: failed to load plugins: {e}");
        }
    }

    for name in collect_handler_names(pipeline) {
        if reg.get_handler(&name).is_none() {
            let n = name.clone();
            reg.handler_value(name, move |_input: Value| {
                let handler_name = n.clone();
                async move {
                    eprintln!("[cruxx] warning: no builtin for '{handler_name}', using stub");
                    Ok(json!({
                        "_stub": handler_name,
                        "confidence": 0.5,
                        "score": 0.5,
                    }))
                }
            });
        }
    }

    reg
}

/// Collect all handler/arm/stage names referenced in the pipeline.
fn collect_handler_names(pipeline: &PipelineDef) -> Vec<String> {
    use cruxx_script::schema::StepDef;
    let mut names = Vec::new();

    for step in &pipeline.steps {
        match step {
            StepDef::Step(node) => {
                names.push(node.handler.clone().unwrap_or_else(|| node.step.clone()));
            }
            StepDef::Delegate(node) => {
                names.push(node.delegate.clone());
            }
            StepDef::Pipe(node) => {
                names.extend(node.stages.iter().map(|a| a.handler_name().to_string()));
            }
            StepDef::JoinAll(node) => {
                names.extend(node.arms.iter().map(|a| a.handler_name().to_string()));
            }
            StepDef::RouteOnConfidence(node) => {
                for route in &node.routes {
                    names.push(route.handler.clone());
                }
            }
            StepDef::Speculate(node) => {
                names.extend(node.arms.iter().map(|a| a.handler_name().to_string()));
            }
        }
    }

    names.sort();
    names.dedup();
    names
}

fn print_trace(cruxx: &Crux<Value>, elapsed: std::time::Duration) {
    println!("Pipeline: {}", cruxx.agent);
    println!(
        "Status:   {}",
        if cruxx.value().is_ok() {
            "OK"
        } else {
            "FAILED"
        }
    );
    println!("Duration: {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    println!("Steps:    {}", cruxx.steps.len());
    println!();

    println!("Trace:");
    for (i, step) in cruxx.steps.iter().enumerate() {
        let status = match step.status {
            StepStatus::Ok => "OK",
            StepStatus::Err => "ERR",
            StepStatus::Rejected => "REJ",
            StepStatus::Skipped => "SKIP",
        };
        let kind = match step.kind {
            StepKind::Plain => "",
            StepKind::Delegation => " [delegate]",
            StepKind::Branch => " [branch]",
            StepKind::Speculation => " [speculate]",
        };
        println!(
            "  {:>2}. [{:>4}] {}{} ({}ms)",
            i + 1,
            status,
            step.name,
            kind,
            step.duration_ms
        );
    }

    println!();
    match cruxx.value() {
        Ok(v) => {
            let pretty = serde_json::to_string_pretty(v).unwrap_or_default();
            println!("Output:\n{pretty}");
        }
        Err(e) => {
            println!("Error: {e}");
        }
    }
}
