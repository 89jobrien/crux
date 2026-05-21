/// crux — pipeline runner and planner for crux-script.
///
/// Subcommands:
///   run   Execute a YAML pipeline
///   plan  Generate a pipeline from a natural language goal
use std::collections::BTreeMap;
use std::io::Read as _;
use std::sync::Arc;
use std::time::Instant;

use clap::{Parser, ValueEnum};
use crux_plugin::bridge::register_plugins;
use crux_plugin::discovery::{PluginDiscovery, TomlFileDiscovery};
use crux_runtime::prelude::*;
use crux_script::{HandlerRegistry, Runner, TargetResolver, schema::PipelineDef};
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
        Cli::Check { pipelines } => cmd_check(&pipelines),
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
        } => cmd_run_dispatch(
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
        ),
        Cli::Plan {
            goal,
            output,
            constraints,
            output_type,
            plugins,
            planner,
        } => cmd_plan(
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

fn cmd_check(paths: &[String]) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut parse_errors = 0u32;
    let mut errors = 0u32;
    let mut warnings = 0u32;

    // Build a registry once with all built-in handlers for validation.
    let empty_pipeline = crux_script::schema::PipelineDef {
        pipeline: String::new(),
        budget: None,
        steps: vec![],
    };
    let registry = rt.block_on(build_registry(&empty_pipeline, None));

    for path in paths {
        // Try as Cruxfile first if it looks like one.
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("\x1b[31merror\x1b[0m: {path}: {e}");
                parse_errors += 1;
                continue;
            }
        };

        if crux_script::is_cruxfile(&contents) {
            let cruxfile = match crux_script::load_cruxfile(&contents) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("\x1b[31merror\x1b[0m: {path}: {e}");
                    parse_errors += 1;
                    continue;
                }
            };

            let report = crux_script::validate_cruxfile(&cruxfile, &registry);
            let target_count = cruxfile.targets.len();

            if report.is_ok() && report.warning_count() == 0 {
                println!(
                    "\x1b[32mok\x1b[0m: {path} (Cruxfile, {target_count} targets, default: {})",
                    cruxfile.default
                );
            } else {
                for diag in &report.diagnostics {
                    let (color, label) = match diag.severity {
                        crux_script::DiagnosticSeverity::Error => {
                            errors += 1;
                            ("\x1b[31m", "error")
                        }
                        crux_script::DiagnosticSeverity::Warning => {
                            warnings += 1;
                            ("\x1b[33m", "warning")
                        }
                    };
                    eprintln!(
                        "{color}{label}\x1b[0m: {path} [{}]: {}",
                        diag.location, diag.message
                    );
                }
            }
            continue;
        }

        let pipeline = match crux_script::load(&contents) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("\x1b[31merror\x1b[0m: {path}: {e}");
                parse_errors += 1;
                continue;
            }
        };

        let report = crux_script::validate_pipeline(&pipeline, &registry);

        let step_count = pipeline.steps.len();
        let handlers = collect_handler_names(&pipeline);

        if report.is_ok() && report.warning_count() == 0 {
            println!(
                "\x1b[32mok\x1b[0m: {path} ({step_count} steps, handlers: {})",
                handlers.join(", ")
            );
        } else {
            for diag in &report.diagnostics {
                let (color, label) = match diag.severity {
                    crux_script::DiagnosticSeverity::Error => {
                        errors += 1;
                        ("\x1b[31m", "error")
                    }
                    crux_script::DiagnosticSeverity::Warning => {
                        warnings += 1;
                        ("\x1b[33m", "warning")
                    }
                };
                eprintln!(
                    "{color}{label}\x1b[0m: {path} [{}]: {}",
                    diag.location, diag.message
                );
            }
        }
    }

    let total_errors = parse_errors + errors;
    if total_errors > 0 || warnings > 0 {
        eprintln!();
        eprintln!(
            "Summary: {} file(s) checked, {} error(s), {} warning(s)",
            paths.len(),
            total_errors,
            warnings
        );
    }

    if total_errors > 0 {
        std::process::exit(1);
    }
}

/// Dispatch between Cruxfile (multi-target) and regular pipeline execution.
#[allow(clippy::too_many_arguments)]
fn cmd_run_dispatch(
    pipeline_arg: Option<&str>,
    target_or_input: Option<&str>,
    target_flag: Option<&str>,
    input_flag: Option<&str>,
    plugins_path: Option<&str>,
    quiet: bool,
    verbose: bool,
    dry_run: bool,
    replay_path: Option<&str>,
    replay_mode_str: &str,
    save_trace_path: Option<&str>,
) {
    // Resolve pipeline path: explicit arg, or discover Cruxfile in cwd.
    let pipeline_path = match pipeline_arg {
        Some("-") => {
            // stdin -- always a regular pipeline
            cmd_run(
                "-",
                target_or_input.or(input_flag),
                plugins_path,
                quiet,
                verbose,
                replay_path,
                replay_mode_str,
                save_trace_path,
            );
            return;
        }
        Some(p) => p.to_string(),
        None => {
            // Discovery: look for Cruxfile in cwd
            if std::path::Path::new("Cruxfile").exists() {
                "Cruxfile".to_string()
            } else {
                eprintln!("error: no pipeline file specified and no Cruxfile found in cwd");
                std::process::exit(1);
            }
        }
    };

    // Try to detect if this is a Cruxfile.
    let contents = std::fs::read_to_string(&pipeline_path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {pipeline_path}: {e}");
        std::process::exit(1);
    });

    if crux_script::is_cruxfile(&contents) {
        let target_name = target_flag.or(target_or_input).map(String::from);

        if dry_run {
            cmd_dry_run_cruxfile(&contents, &pipeline_path, target_name.as_deref());
        } else {
            cmd_run_cruxfile(
                &contents,
                &pipeline_path,
                target_name.as_deref(),
                plugins_path,
                quiet,
                verbose,
                save_trace_path,
            );
        }
    } else {
        // Regular pipeline. target_or_input is actually an input file.
        if dry_run {
            cmd_dry_run_pipeline(&contents, &pipeline_path);
        } else {
            let input_path = input_flag.or(target_or_input);
            cmd_run(
                &pipeline_path,
                input_path,
                plugins_path,
                quiet,
                verbose,
                replay_path,
                replay_mode_str,
                save_trace_path,
            );
        }
    }
}

/// Print Cruxfile execution plan without running.
fn cmd_dry_run_cruxfile(contents: &str, path: &str, target_name: Option<&str>) {
    let cruxfile = crux_script::load_cruxfile(contents).unwrap_or_else(|e| {
        eprintln!("error: failed to parse {path}: {e}");
        std::process::exit(1);
    });

    let target = target_name.unwrap_or(&cruxfile.default);

    let resolver = TargetResolver::new(&cruxfile).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    let order = resolver.execution_order(target).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    println!("Cruxfile: {} (target: {target})", cruxfile.project);
    println!("Execution order: {}\n", order.join(" -> "));

    for (i, &name) in order.iter().enumerate() {
        let target_def = &cruxfile.targets[name];
        let budget_info = target_def
            .budget
            .as_ref()
            .or(cruxfile.budget.as_ref())
            .map(|b| format!(" (budget: {b:?})"))
            .unwrap_or_default();

        if target_def.steps.is_empty() {
            println!("  {:>2}. {name} (aggregation target){budget_info}", i + 1);
        } else {
            let tmp = PipelineDef {
                pipeline: name.to_string(),
                budget: None,
                steps: target_def.steps.clone(),
            };
            let handlers = collect_handler_names(&tmp);
            println!(
                "  {:>2}. {name} ({} steps: {}){budget_info}",
                i + 1,
                target_def.steps.len(),
                handlers.join(", ")
            );
        }
    }
}

/// Print pipeline execution plan without running.
fn cmd_dry_run_pipeline(contents: &str, path: &str) {
    let pipeline = crux_script::load(contents).unwrap_or_else(|e| {
        eprintln!("error: failed to parse {path}: {e}");
        std::process::exit(1);
    });

    let handlers = collect_handler_names(&pipeline);
    println!(
        "Pipeline: {} ({} steps)\n",
        pipeline.pipeline,
        pipeline.steps.len()
    );
    for (i, name) in handlers.iter().enumerate() {
        println!("  {:>2}. {name}", i + 1);
    }
}

/// Run a Cruxfile: resolve target, execute dependency chain.
fn cmd_run_cruxfile(
    contents: &str,
    path: &str,
    target_name: Option<&str>,
    plugins_path: Option<&str>,
    quiet: bool,
    verbose: bool,
    save_trace_path: Option<&str>,
) {
    let cruxfile = crux_script::load_cruxfile(contents).unwrap_or_else(|e| {
        eprintln!("error: failed to parse {path}: {e}");
        std::process::exit(1);
    });

    let target = target_name.unwrap_or(&cruxfile.default);

    let resolver = TargetResolver::new(&cruxfile).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    let order = resolver.execution_order(target).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    if verbose {
        eprintln!(
            "[crux] Cruxfile: project={}, target={target}, plan: {}",
            cruxfile.project,
            order.join(" -> ")
        );
    }

    // Build registry once using an empty pipeline (all handlers registered).
    let rt = tokio::runtime::Runtime::new().unwrap();
    let empty_pipeline = PipelineDef {
        pipeline: String::new(),
        budget: None,
        steps: vec![],
    };
    let registry = rt.block_on(build_registry(&empty_pipeline, plugins_path));

    // Also register any handlers referenced in all targets.
    let mut full_reg = registry;
    for (_, tgt) in &cruxfile.targets {
        let tmp_pipeline = PipelineDef {
            pipeline: String::new(),
            budget: None,
            steps: tgt.steps.clone(),
        };
        for name in collect_handler_names(&tmp_pipeline) {
            if full_reg.get_handler(&name).is_none() {
                let n = name.clone();
                full_reg.handler_value(name, move |_input: Value| {
                    let handler_name = n.clone();
                    async move {
                        eprintln!("[crux] warning: no builtin for '{handler_name}', using stub");
                        Ok(json!({
                            "_stub": handler_name,
                            "confidence": 0.5,
                            "score": 0.5,
                        }))
                    }
                });
            }
        }
    }

    let runner = Runner::new(Arc::new(full_reg));
    let mut failed = false;
    let mut skipped: Vec<&str> = Vec::new();

    let start = Instant::now();

    for &target_name in &order {
        if failed {
            skipped.push(target_name);
            continue;
        }

        let target_def = &cruxfile.targets[target_name];
        let budget = target_def.budget.as_ref().or(cruxfile.budget.as_ref());

        if verbose {
            eprintln!("[crux] running target: {target_name}");
        }

        let target_start = Instant::now();
        let crux = rt.block_on(runner.run_target(target_def, target_name, budget));
        let target_elapsed = target_start.elapsed();
        let is_ok = crux.value().is_ok();
        if !quiet {
            let icon = if is_ok {
                "\x1b[32mok\x1b[0m"
            } else {
                "\x1b[31mERR\x1b[0m"
            };
            let elapsed_ms = target_elapsed.as_millis();
            eprintln!("  [{icon}] {target_name} ({elapsed_ms}ms)");
        }

        if verbose {
            let status = if is_ok { "OK" } else { "FAILED" };
            eprintln!(
                "[crux]   {target_name}: {status} ({} steps)",
                crux.steps.len()
            );
        }

        if let Err(e) = crux.value() {
            eprintln!("[crux] target '{target_name}' failed: {e}");
            failed = true;
        }

        if let Some(trace_dir) = save_trace_path {
            let trace_file = format!("{trace_dir}.{target_name}.json");
            let trace_json =
                serde_json::to_string_pretty(&crux).expect("failed to serialize trace");
            std::fs::write(&trace_file, trace_json).expect("failed to write trace file");
            if !quiet {
                eprintln!("[crux] trace saved to {trace_file}");
            }
        }
    }

    let elapsed = start.elapsed();
    let total = order.len();
    let skipped_count = skipped.len();
    let failed_count = if failed { 1 } else { 0 };
    let ok_count = total - skipped_count - failed_count;

    if !skipped.is_empty() && !quiet {
        eprintln!("[crux] skipped due to failure: {}", skipped.join(", "));
    }

    if !quiet {
        let elapsed_str = if elapsed.as_secs() >= 1 {
            format!("{:.1}s", elapsed.as_secs_f64())
        } else {
            format!("{}ms", elapsed.as_millis())
        };
        let status = if failed {
            format!("{ok_count}/{total} targets OK, {failed_count} failed, {skipped_count} skipped")
        } else {
            format!("{ok_count}/{total} targets OK")
        };
        eprintln!(
            "Cruxfile: {} [{target}] {status} ({elapsed_str})",
            cruxfile.project
        );
    }

    if verbose {
        eprintln!("[crux] total: {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    }

    if failed {
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_run(
    pipeline_path: &str,
    input_path: Option<&str>,
    plugins_path: Option<&str>,
    quiet: bool,
    verbose: bool,
    replay_path: Option<&str>,
    replay_mode_str: &str,
    save_trace_path: Option<&str>,
) {
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
        crux_script::load(&buf).expect("failed to parse pipeline from stdin")
    } else {
        crux_script::load_file(pipeline_path).expect("failed to load pipeline")
    };

    warn_missing_env(&pipeline);

    let replay_mode = match replay_mode_str {
        "lenient" => ReplayMode::Lenient,
        _ => ReplayMode::Strict,
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let registry = rt.block_on(build_registry(&pipeline, plugins_path));
    let runner = Runner::new(Arc::new(registry));

    let previous: Option<Crux<Value>> = replay_path.map(|path| {
        let contents = std::fs::read_to_string(path).expect("failed to read replay trace");
        serde_json::from_str(&contents).expect("invalid replay trace JSON")
    });

    let start = Instant::now();
    let crux = if let Some(ref prev) = previous {
        rt.block_on(runner.run_with_replay(&pipeline, input, prev, replay_mode))
    } else {
        rt.block_on(runner.run(&pipeline, input))
    };
    let elapsed = start.elapsed();

    if let Some(path) = save_trace_path {
        let trace_json = serde_json::to_string_pretty(&crux).expect("failed to serialize trace");
        std::fs::write(path, trace_json).expect("failed to write trace file");
        if !quiet {
            eprintln!("[crux] trace saved to {path}");
        }
    }

    if verbose {
        print_trace(&crux, elapsed);
    } else if !quiet {
        match crux.value() {
            Ok(v) => println!("{}", serde_json::to_string(v).unwrap_or_default()),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    } else if let Err(e) = crux.value() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Inline rule planner — avoids circular dep with crux-planner (which depends
// on crux-agentic for its baml feature). Mirrors crux-planner::RulePlanner.
// ---------------------------------------------------------------------------

/// A rule mapping goal keywords → handler step sequence.
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

// ---------------------------------------------------------------------------

fn cmd_plan(
    goal: &str,
    output: Option<&str>,
    constraints: Option<&str>,
    output_type: &OutputType,
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

#[cfg(feature = "baml")]
fn cmd_plan_llm(
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
    _output_type: &OutputType,
    _plugins_path: Option<&str>,
) {
    eprintln!(
        "crux plan --planner llm requires --features baml. \
         Run: cargo build --features baml"
    );
    std::process::exit(1);
}

#[cfg(feature = "baml")]
fn format_plan_output(yaml: &str, goal: &str, output_type: &OutputType) -> String {
    match output_type {
        OutputType::Yaml => yaml.to_string(),

        OutputType::Json => {
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

        OutputType::Pretty => {
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

        OutputType::DryRun => {
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

        OutputType::Handoff => {
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
            "[crux] warning: pipeline uses llm:: handlers but neither \
             OPENAI_API_KEY nor ANTHROPIC_API_KEY is set"
        );
        eprintln!(
            "[crux] hint: copy .env.example to .env and configure, \
             or use `dotenvx run -- crux run ...`"
        );
    }
}

fn resolve_plugins_path(plugins_path: Option<&str>) -> String {
    plugins_path.map(String::from).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.crux/plugins.toml")
    })
}

/// Build a registry seeded with all crux-agentic built-in handlers.
async fn build_registry(pipeline: &PipelineDef, plugins_path: Option<&str>) -> HandlerRegistry {
    let disc = TomlFileDiscovery::new(resolve_plugins_path(plugins_path));
    let entries = disc.discover().unwrap_or_default();
    let manifest = crux_plugin::manifest::PluginManifest { plugin: entries };

    let plugin_handler_descs: Vec<String> = manifest
        .plugin
        .iter()
        .map(|p| format!("{}::* -- plugin (see plugins.toml)", p.name))
        .collect();

    let mut reg = HandlerRegistry::new();
    crux_agentic::register_all_with_plugins(&mut reg, plugin_handler_descs);

    if !manifest.plugin.is_empty()
        && let Err(e) = register_plugins(&mut reg, &manifest.plugin).await
    {
        eprintln!("[crux] warning: failed to load plugins: {e}");
    }

    for name in collect_handler_names(pipeline) {
        if reg.get_handler(&name).is_none() {
            let n = name.clone();
            reg.handler_value(name, move |_input: Value| {
                let handler_name = n.clone();
                async move {
                    // TODO(#64): fail or warn loudly when a pipeline references an unregistered handler
                    eprintln!("[crux] warning: no builtin for '{handler_name}', using stub");
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
    use crux_script::schema::StepDef;
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

fn print_trace(crux: &Crux<Value>, elapsed: std::time::Duration) {
    println!("Pipeline: {}", crux.agent);
    println!(
        "Status:   {}",
        if crux.value().is_ok() { "OK" } else { "FAILED" }
    );
    println!("Duration: {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    println!("Steps:    {}", crux.steps.len());
    println!();

    println!("Trace:");
    for (i, step) in crux.steps.iter().enumerate() {
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
    match crux.value() {
        Ok(v) => {
            let pretty = serde_json::to_string_pretty(v).unwrap_or_default();
            println!("Output:\n{pretty}");
        }
        Err(e) => {
            println!("Error: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Empty goal returns the default fallback — must not panic and must be non-empty.
        let steps = rule_planner_steps("");
        assert!(
            !steps.is_empty(),
            "rule planner must return default steps for empty goal"
        );
    }
}
