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
#[command(
    name = "crux",
    about = "crux pipeline runner and planner",
    after_help = "Cruxfile shorthand:\n  crux <target>       run a Cruxfile target -- same as `crux run --target <target>`\n  crux <file.crux>    run a pipeline file   -- same as `crux run <file.crux>`\n\nSubcommand names (list, check, run, plan, help) always win over a Cruxfile\ntarget of the same name; reach those with `crux run --target <name>`."
)]
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

/// Subcommand names that take precedence over the Cruxfile target shorthand.
const SUBCOMMANDS: &[&str] = &["list", "check", "run", "plan", "help"];

/// Rewrite `crux <target> [args..]` into `crux run --target <target> [args..]`.
///
/// A first argument naming a subcommand, starting with `-`, or absent entirely
/// is left alone, so every existing invocation parses exactly as before. A
/// first argument that names an existing file becomes `run <file>` instead, so
/// `crux path/to/pipeline.crux` runs that pipeline directly.
///
/// `is_file` is injected so the rewrite is testable without touching disk.
fn normalize_args(args: Vec<String>, is_file: impl Fn(&str) -> bool) -> Vec<String> {
    let Some(first) = args.get(1).cloned() else {
        return args;
    };
    if first.starts_with('-') || SUBCOMMANDS.contains(&first.as_str()) {
        return args;
    }

    let mut out = vec![args[0].clone(), "run".to_string()];
    if is_file(&first) {
        out.push(first);
    } else {
        out.push("--target".to_string());
        out.push(first);
    }
    out.extend(args.into_iter().skip(2));
    out
}

/// Restore the default SIGPIPE disposition.
///
/// Rust's runtime ignores SIGPIPE, so a write to a closed pipe surfaces as an
/// `io::Error` that `println!` turns into a panic. That makes the ordinary
/// `crux lint -n | head` print a backtrace. Restoring the default lets the
/// process die quietly on the signal, the way every other CLI does.
#[cfg(unix)]
fn restore_sigpipe() {
    // SAFETY: `signal` with SIG_DFL is async-signal-safe and this runs before
    // any thread is spawned.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_sigpipe() {}

fn main() {
    restore_sigpipe();

    let argv = normalize_args(std::env::args().collect(), |p| {
        std::path::Path::new(p).is_file()
    });
    let cli = Cli::parse_from(argv);

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
        } => run::cmd_run_dispatch(&run::RunConfig {
            pipeline_arg: pipeline.as_deref(),
            target_or_input: target_or_input.as_deref(),
            target_flag: target.as_deref(),
            input_flag: input.as_deref(),
            plugins_path: plugins.as_deref(),
            quiet,
            verbose,
            dry_run,
            replay_path: replay.as_deref(),
            replay_mode_str: &replay_mode,
            save_trace_path: save_trace.as_deref(),
            strict,
        }),
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
    use super::normalize_args;
    use super::plan::*;

    /// Normalize an argv vector against a cwd where no file exists.
    fn norm(args: &[&str]) -> Vec<String> {
        normalize_args(args.iter().map(|s| s.to_string()).collect(), |_| false)
    }

    /// Normalize an argv vector against a cwd where exactly `file` exists.
    fn norm_with_file(args: &[&str], file: &str) -> Vec<String> {
        let file = file.to_string();
        normalize_args(args.iter().map(|s| s.to_string()).collect(), move |p| {
            p == file
        })
    }

    #[test]
    fn bare_target_rewrites_to_run_with_target_flag() {
        assert_eq!(norm(&["crux", "fmt"]), ["crux", "run", "--target", "fmt"]);
    }

    #[test]
    fn bare_target_preserves_trailing_flags() {
        assert_eq!(
            norm(&["crux", "lint", "-v", "--save-trace", "t.json"]),
            [
                "crux",
                "run",
                "--target",
                "lint",
                "-v",
                "--save-trace",
                "t.json"
            ]
        );
    }

    #[test]
    fn hyphenated_target_is_not_mistaken_for_a_flag() {
        assert_eq!(
            norm(&["crux", "lint-crux"]),
            ["crux", "run", "--target", "lint-crux"]
        );
    }

    #[test]
    fn known_subcommands_are_left_alone() {
        for sub in ["list", "check", "run", "plan", "help"] {
            assert_eq!(
                norm(&["crux", sub, "x"]),
                ["crux", sub, "x"],
                "subcommand '{sub}' must not be rewritten"
            );
        }
    }

    #[test]
    fn leading_flag_is_left_alone() {
        assert_eq!(norm(&["crux", "--help"]), ["crux", "--help"]);
        assert_eq!(norm(&["crux", "-h"]), ["crux", "-h"]);
    }

    #[test]
    fn no_args_is_left_alone() {
        assert_eq!(norm(&["crux"]), ["crux"]);
    }

    #[test]
    fn existing_file_becomes_a_pipeline_run_not_a_target() {
        assert_eq!(
            norm_with_file(
                &["crux", "examples/showcase.crux", "-v"],
                "examples/showcase.crux"
            ),
            ["crux", "run", "examples/showcase.crux", "-v"]
        );
    }

    #[test]
    fn missing_file_path_falls_through_to_target() {
        // A path that does not exist is still treated as a target name, so the
        // error surfaces as "unknown target" with the target list.
        assert_eq!(
            norm(&["crux", "nope.crux"]),
            ["crux", "run", "--target", "nope.crux"]
        );
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
