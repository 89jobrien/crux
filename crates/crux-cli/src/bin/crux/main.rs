//! Argument parsing and command dispatch for the `crux` pipeline CLI.

/// crux — pipeline runner and planner for crux-script.
///
/// Subcommands:
///   check Validate one or more YAML pipelines
///   run   Execute a YAML pipeline
///   plan  Generate a pipeline from a natural language goal
use std::collections::BTreeMap;

use clap::{Parser, ValueEnum};

mod check;
mod doctor;
mod handlers;
mod init;
mod output;
mod plan;
mod registry;
mod replay_debug;
mod run;
mod schema;
mod test_cmd;
mod trace;

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
#[command(name = "crux", about = "crux pipeline runner and planner")]
enum Cli {
    /// List discovered .crux pipeline files under a directory
    List {
        /// Root directory to scan (default: current directory)
        #[arg(default_value = ".")]
        root: String,
    },
    /// Generate documentation from registered handler metadata
    Handlers {
        /// Catalog serialization format
        #[arg(long, value_enum, default_value_t = handlers::HandlerFormat::Markdown)]
        format: handlers::HandlerFormat,
        /// Path to plugins.toml (default: ~/.crux/plugins.toml)
        #[arg(long)]
        plugins: Option<String>,
    },
    /// Scaffold a new Crux Rust project and sample pipeline
    Init {
        /// Destination directory
        #[arg(default_value = ".")]
        path: String,
    },
    /// Diagnose local capabilities and configuration without exposing secrets
    Doctor {
        /// Path to plugins.toml (default: ~/.crux/plugins.toml)
        #[arg(long)]
        plugins: Option<String>,
    },
    /// Run a JSON pipeline fixture with deterministic mocked handlers
    Test {
        /// Fixture JSON path
        fixture: String,
    },
    /// Inspect serialized replay traces and explain drift
    ReplayDebug {
        /// Serialized Crux trace
        trace: String,
        /// Show one step in detail
        #[arg(long)]
        step: Option<usize>,
        /// Compare against another trace
        #[arg(long)]
        compare: Option<String>,
    },
    /// Explore a serialized Crux execution trace
    Trace {
        /// Serialized Crux trace
        trace: String,
        /// Filter by step status
        #[arg(long)]
        status: Option<String>,
        /// Filter by step kind
        #[arg(long)]
        kind: Option<String>,
        /// Filter by minimum confidence
        #[arg(long)]
        min_confidence: Option<f32>,
        /// Export the causal graph as Mermaid
        #[arg(long)]
        mermaid: bool,
    },
    /// Compile-check one or more .crux pipelines or Cruxfiles
    Check {
        /// Pipeline/Cruxfile paths to check
        #[arg(required = true)]
        paths: Vec<String>,
        /// Require complete contracts and reject dynamic boundaries
        #[arg(short = 'S', long)]
        strict: bool,
        /// Path to plugins.toml (default: ~/.crux/plugins.toml)
        #[arg(long)]
        plugins: Option<String>,
        /// Emit diagnostics as JSON for editors and CI
        #[arg(long)]
        json: bool,
    },
    /// Export the JSON Schema for .crux pipeline definitions
    Schema {
        /// Serialization format
        #[arg(long, value_enum, default_value_t = schema::SchemaFormat::Json)]
        format: schema::SchemaFormat,
        /// Write the schema to a file for editor configuration
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Execute a .crux pipeline or Cruxfile ("-" reads from stdin)
    Run {
        /// Pipeline/Cruxfile path ("-" for stdin). If omitted, discovers Cruxfile in cwd.
        pipeline: Option<String>,
        /// Optional: target name (for Cruxfile) or input JSON file (for pipeline)
        target_or_input: Option<String>,
        /// Validate the pipeline without executing it
        #[arg(long)]
        check: bool,
        /// Target to run from a Cruxfile (alternative to positional)
        #[arg(long)]
        target: Option<String>,
        /// Input JSON file (use this when both target and input are needed)
        #[arg(long)]
        input: Option<String>,
        /// Path to plugins.toml (default: ~/.crux/plugins.toml)
        #[arg(long)]
        plugins: Option<String>,
        /// Suppress all output except errors
        #[arg(short, long, conflicts_with_all = ["summary", "verbose", "json"])]
        quiet: bool,
        /// Explicitly select the default concise human-readable summary
        #[arg(long, conflicts_with_all = ["quiet", "verbose", "json"])]
        summary: bool,
        /// Emit only the compact machine-readable JSON result
        #[arg(long, conflicts_with_all = ["quiet", "summary", "verbose"])]
        json: bool,
        /// Show pipeline metadata, full trace, and display-aware humanized output
        #[arg(short, long, conflicts_with_all = ["quiet", "summary", "json"])]
        verbose: bool,
        /// Print execution plan without running anything
        #[arg(short = 'n', long)]
        dry_run: bool,
        /// Replay from a previous trace JSON file (skip cached steps)
        #[arg(long)]
        replay: Option<String>,
        /// Replay matching mode: "strict" (default) or "lenient"
        #[arg(long, default_value = "strict")]
        replay_mode: String,
        /// Error on unregistered handlers instead of injecting stubs
        #[arg(short = 'S', long)]
        strict: bool,
        /// Override automatic trace path (Cruxfiles append .<target>.json)
        #[arg(long)]
        save_trace: Option<String>,
    },
    /// Generate a pipeline from a natural language goal
    Plan {
        /// Natural language goal
        #[arg(long)]
        goal: String,
        /// Output file (stdout if omitted)
        #[arg(short, long)]
        output: Option<String>,
        /// Optional constraints (llm planner only)
        #[arg(long)]
        constraints: Option<String>,
        /// Output format
        #[arg(long, value_enum, default_value_t = OutputType::Yaml)]
        output_type: OutputType,
        /// Path to plugins.toml (default: ~/.crux/plugins.toml)
        #[arg(long)]
        plugins: Option<String>,
        /// Planner backend: "rule" (default, no API key needed) or "llm" (requires --features baml)
        #[arg(long, default_value = "rule")]
        planner: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli {
        Cli::List { root } => cmd_list(&root),
        Cli::Handlers { format, plugins } => {
            handlers::cmd_handlers(format, plugins.as_deref());
        }
        Cli::Init { path } => init::cmd_init(&path),
        Cli::Doctor { plugins } => doctor::cmd_doctor(plugins.as_deref()),
        Cli::Test { fixture } => test_cmd::cmd_test(&fixture),
        Cli::ReplayDebug {
            trace,
            step,
            compare,
        } => replay_debug::cmd_replay_debug(&trace, step, compare.as_deref()),
        Cli::Trace {
            trace,
            status,
            kind,
            min_confidence,
            mermaid,
        } => trace::cmd_trace(
            &trace,
            status.as_deref(),
            kind.as_deref(),
            min_confidence,
            mermaid,
        ),
        Cli::Check {
            paths,
            strict,
            plugins,
            json,
        } => check::cmd_check_with_options(&paths, plugins.as_deref(), strict, json),
        Cli::Schema { format, output } => schema::cmd_schema(format, output.as_deref()),
        Cli::Run {
            pipeline,
            target_or_input,
            check,
            target,
            input,
            plugins,
            quiet,
            summary,
            json,
            verbose,
            dry_run,
            replay,
            replay_mode,
            save_trace,
            strict,
        } => {
            if check
                && let Some(path) = pipeline.as_ref()
                && path != "-"
            {
                check::cmd_check_with_options(
                    std::slice::from_ref(path),
                    plugins.as_deref(),
                    strict,
                    false,
                );
                return;
            }
            run::cmd_run_dispatch(&run::RunConfig {
                pipeline_arg: pipeline.as_deref(),
                target_or_input: target_or_input.as_deref(),
                check,
                target_flag: target.as_deref(),
                input_flag: input.as_deref(),
                plugins_path: plugins.as_deref(),
                quiet,
                summary,
                json,
                verbose,
                dry_run,
                replay_path: replay.as_deref(),
                replay_mode_str: &replay_mode,
                save_trace_path: save_trace.as_deref(),
                strict,
            });
        }
        Cli::Plan {
            goal,
            output,
            constraints,
            output_type,
            plugins,
            planner,
        } => plan::cmd_plan(
            &goal,
            output.as_deref(),
            constraints.as_deref(),
            &output_type,
            plugins.as_deref(),
            &planner,
        ),
    }
}

fn cmd_list(root: &str) {
    let root_path = std::path::Path::new(root);
    let pipelines = crux_agentic::discover::discover_pipelines(root_path);

    if pipelines.is_empty() {
        eprintln!("No .crux files found under {root}");
        return;
    }

    // Group by parent directory for readability.
    let mut by_dir: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in &pipelines {
        let dir = path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| ".".to_string());
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        by_dir.entry(dir).or_default().push(name);
    }

    for (dir, files) in &by_dir {
        println!("{dir}/");
        for f in files {
            println!("  {f}");
        }
    }

    eprintln!("\n{} pipeline(s) found", pipelines.len());
}

#[cfg(test)]
mod tests {
    use super::{Cli, plan::*};
    use clap::Parser;

    #[test]
    fn run_accepts_json_output_mode() {
        let cli = Cli::try_parse_from(["crux", "run", "pipeline.crux", "--json"]);
        assert!(matches!(cli, Ok(Cli::Run { json: true, .. })));
    }

    #[test]
    fn run_rejects_conflicting_output_modes() {
        let cli = Cli::try_parse_from(["crux", "run", "pipeline.crux", "--json", "--verbose"]);
        assert!(cli.is_err());
    }

    #[test]
    fn plan_subcommand_with_rule_planner_prints_steps() {
        let steps = rule_planner_steps("fetch data");
        assert!(
            !steps.is_empty(),
            "rule planner must return at least one step for 'fetch data'"
        );
        assert!(
            steps.contains(&"http::get".to_string()),
            "expected http::get for goal containing 'fetch', got: {steps:?}"
        );
    }

    #[test]
    fn plan_subcommand_rule_planner_summarize() {
        let steps = rule_planner_steps("summarize the report");
        assert!(
            steps.contains(&"llm::complete".to_string()),
            "expected llm::complete for goal containing 'summarize', got: {steps:?}"
        );
    }

    #[test]
    fn plan_subcommand_goal_required() {
        let steps = rule_planner_steps("");
        assert!(
            !steps.is_empty(),
            "rule planner must return default steps for empty goal"
        );
    }
}
