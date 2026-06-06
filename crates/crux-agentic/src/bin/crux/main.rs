/// crux — pipeline runner and planner for crux-script.
///
/// Subcommands:
///   run   Execute a YAML pipeline
///   plan  Generate a pipeline from a natural language goal
use std::collections::BTreeMap;

use clap::{Parser, ValueEnum};

mod check;
mod plan;
mod registry;
mod run;

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
    /// Validate a .crux pipeline file without executing it
    Check {
        /// Pipeline file(s) to validate
        #[arg(required = true)]
        pipelines: Vec<String>,
    },
    /// Execute a .crux pipeline or Cruxfile ("-" reads from stdin)
    Run {
        /// Pipeline/Cruxfile path ("-" for stdin). If omitted, discovers Cruxfile in cwd.
        pipeline: Option<String>,
        /// Optional: target name (for Cruxfile) or input JSON file (for pipeline)
        target_or_input: Option<String>,
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
        #[arg(short, long)]
        quiet: bool,
        /// Show full trace envelope (pipeline info, steps, timing)
        #[arg(short, long)]
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
        /// Save the execution trace to a JSON file (replayable)
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
        Cli::Check { pipelines } => check::cmd_check(&pipelines),
        Cli::Run {
            pipeline,
            target_or_input,
            target,
            input,
            plugins,
            quiet,
            verbose,
            dry_run,
            replay,
            replay_mode,
            save_trace,
            strict,
        } => run::cmd_run_dispatch(
            pipeline.as_deref(),
            target_or_input.as_deref(),
            target.as_deref(),
            input.as_deref(),
            plugins.as_deref(),
            quiet,
            verbose,
            dry_run,
            replay.as_deref(),
            &replay_mode,
            save_trace.as_deref(),
            strict,
        ),
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
    use super::plan::*;

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
