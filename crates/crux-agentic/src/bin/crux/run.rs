use std::io::Read as _;
use std::sync::Arc;
use std::time::Instant;

use crux_runtime::prelude::*;
use crux_script::{TargetResolver, schema::CruxfileDef, schema::PipelineDef};
use serde_json::Value;

use crate::registry::{
    build_registry, collect_handler_names, print_trace, register_stub_handler, warn_missing_env,
};

/// Shared config for the `run` subcommand, replacing positional arg sprawl.
pub struct RunConfig<'a> {
    pub pipeline_arg: Option<&'a str>,
    pub target_or_input: Option<&'a str>,
    pub target_flag: Option<&'a str>,
    pub input_flag: Option<&'a str>,
    pub plugins_path: Option<&'a str>,
    pub quiet: bool,
    pub verbose: bool,
    pub dry_run: bool,
    pub replay_path: Option<&'a str>,
    pub replay_mode_str: &'a str,
    pub save_trace_path: Option<&'a str>,
    pub strict: bool,
}

/// Resolve the pipeline path from the config, or discover `Cruxfile` in cwd.
///
/// Returns `None` when the pipeline arg is `"-"` (stdin), indicating the caller
/// should read from stdin directly. Returns `Some(path)` for a named file.
/// Exits the process if no path is discoverable.
fn resolve_pipeline_path(pipeline_arg: Option<&str>) -> Option<String> {
    match pipeline_arg {
        Some("-") => None, // caller handles stdin
        Some(p) => Some(p.to_string()),
        None => {
            if std::path::Path::new("Cruxfile").exists() {
                Some("Cruxfile".to_string())
            } else {
                eprintln!("error: no pipeline file specified and no Cruxfile found in cwd");
                std::process::exit(1);
            }
        }
    }
}

/// Select the effective target name from config fields, in priority order.
///
/// Pure: no I/O. Returns `None` when no target was specified.
fn select_target_name<'a>(cfg: &'a RunConfig<'_>) -> Option<&'a str> {
    cfg.target_flag.or(cfg.target_or_input)
}

/// Dispatch to the appropriate execution path given already-loaded file contents.
///
/// All I/O (file read, stdin) is done before this call; this function is pure
/// dispatch over the parsed `contents` and config.
fn dispatch_on_contents(contents: &str, pipeline_path: &str, cfg: &RunConfig<'_>) {
    if crux_script::is_cruxfile(contents) {
        // Both are single-pipeline notions: targets run with a null input, and
        // replay matches a trace against one pipeline's steps.
        if cfg.replay_path.is_some() {
            eprintln!("[crux] warning: --replay ignored: Cruxfile targets are not replayable");
        }
        if let Some(input) = cfg.input_flag {
            eprintln!(
                "[crux] warning: --input {input} ignored: Cruxfile targets take no JSON input"
            );
        }
        let target_name = select_target_name(cfg).map(String::from);
        if cfg.dry_run {
            cmd_dry_run_cruxfile(contents, pipeline_path, target_name.as_deref());
        } else {
            cmd_run_cruxfile(contents, pipeline_path, target_name.as_deref(), cfg);
        }
    } else {
        // Regular pipeline. target_or_input is the input file, not a target.
        if let Some(t) = cfg.target_flag {
            eprintln!(
                "[crux] warning: --target {t} ignored: {pipeline_path} is a pipeline, not a Cruxfile"
            );
        }
        if cfg.dry_run {
            cmd_dry_run_pipeline(contents, pipeline_path);
        } else {
            let input_path = cfg.input_flag.or(cfg.target_or_input);
            cmd_run(pipeline_path, input_path, cfg);
        }
    }
}

/// Dispatch between Cruxfile (multi-target) and regular pipeline execution.
pub fn cmd_run_dispatch(cfg: &RunConfig<'_>) {
    let Some(pipeline_path) = resolve_pipeline_path(cfg.pipeline_arg) else {
        // stdin path — always a regular pipeline
        cmd_run("-", cfg.target_or_input.or(cfg.input_flag), cfg);
        return;
    };

    let contents = std::fs::read_to_string(&pipeline_path)
        .unwrap_or_else(|e| die(&format!("cannot read {pipeline_path}"), e));

    dispatch_on_contents(&contents, &pipeline_path, cfg);
}

/// Print an error and exit 1.
///
/// Bad input from the user -- an unparseable pipeline, malformed JSON, an
/// unwritable trace path -- is a normal outcome for a CLI, not a bug. These
/// used to be `expect` calls, which met a typo with a panic and a backtrace.
fn die(context: &str, err: impl std::fmt::Display) -> ! {
    eprintln!("error: {context}: {err}");
    std::process::exit(1);
}

/// Resolve a target's execution order, listing the available targets on failure.
///
/// With the bare-target shorthand (`crux <target>`) a typo is the common error,
/// so the target list is worth printing rather than just the parse failure.
fn execution_order_or_exit<'a>(
    resolver: &'a TargetResolver,
    cruxfile: &'a CruxfileDef,
    target: &'a str,
) -> Vec<&'a str> {
    resolver.execution_order(target).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        let names: Vec<&str> = cruxfile.targets.keys().map(String::as_str).collect();
        eprintln!("available targets: {}", names.join(", "));
        std::process::exit(1);
    })
}

/// Print Cruxfile execution plan without running.
fn cmd_dry_run_cruxfile(contents: &str, path: &str, target_name: Option<&str>) {
    let cruxfile = crux_script::load_cruxfile(contents)
        .unwrap_or_else(|e| die(&format!("cannot parse {path}"), e));

    let target = target_name.unwrap_or(&cruxfile.default);

    let resolver = TargetResolver::new(&cruxfile).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    let order = execution_order_or_exit(&resolver, &cruxfile, target);

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
    let pipeline =
        crux_script::load(contents).unwrap_or_else(|e| die(&format!("cannot parse {path}"), e));

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
fn cmd_run_cruxfile(contents: &str, path: &str, target_name: Option<&str>, cfg: &RunConfig<'_>) {
    let plugins_path = cfg.plugins_path;
    let quiet = cfg.quiet;
    let verbose = cfg.verbose;
    let save_trace_path = cfg.save_trace_path;
    let strict = cfg.strict;
    let cruxfile = crux_script::load_cruxfile(contents)
        .unwrap_or_else(|e| die(&format!("cannot parse {path}"), e));

    let target = target_name.unwrap_or(&cruxfile.default);

    let resolver = TargetResolver::new(&cruxfile).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    let order = execution_order_or_exit(&resolver, &cruxfile, target);

    if verbose {
        eprintln!(
            "[crux] Cruxfile: project={}, target={target}, plan: {}",
            cruxfile.project,
            order.join(" -> ")
        );
    }

    // Build registry once using an empty pipeline (all handlers registered).
    let rt =
        tokio::runtime::Runtime::new().unwrap_or_else(|e| die("cannot start the tokio runtime", e));
    let empty_pipeline = PipelineDef {
        pipeline: String::new(),
        budget: None,
        steps: vec![],
    };
    let registry = rt.block_on(build_registry(&empty_pipeline, plugins_path, false));

    // Also register any handlers referenced in all targets.
    let mut full_reg = registry;
    let mut unregistered: Vec<String> = Vec::new();
    for (_, tgt) in &cruxfile.targets {
        let tmp_pipeline = PipelineDef {
            pipeline: String::new(),
            budget: None,
            steps: tgt.steps.clone(),
        };
        for name in collect_handler_names(&tmp_pipeline) {
            if full_reg.get_handler(&name).is_none() {
                if strict {
                    if !unregistered.contains(&name) {
                        unregistered.push(name);
                    }
                } else {
                    register_stub_handler(&mut full_reg, name);
                }
            }
        }
    }

    if !unregistered.is_empty() {
        eprintln!(
            "[crux] error: --strict mode: unregistered handlers: {}",
            unregistered.join(", ")
        );
        std::process::exit(1);
    }

    let runner = crux_script::Runner::new(Arc::new(full_reg));
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
            let trace_json = serde_json::to_string_pretty(&crux)
                .unwrap_or_else(|e| die("cannot serialize trace", e));
            if let Err(e) = std::fs::write(&trace_file, trace_json) {
                die(&format!("cannot write {trace_file}"), e);
            }
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

fn cmd_run(pipeline_path: &str, input_path: Option<&str>, cfg: &RunConfig<'_>) {
    let plugins_path = cfg.plugins_path;
    let quiet = cfg.quiet;
    let verbose = cfg.verbose;
    let replay_path = cfg.replay_path;
    let replay_mode_str = cfg.replay_mode_str;
    let save_trace_path = cfg.save_trace_path;
    let strict = cfg.strict;
    let input: Value = if let Some(path) = input_path {
        let contents = std::fs::read_to_string(path)
            .unwrap_or_else(|e| die(&format!("cannot read input file {path}"), e));
        serde_json::from_str(&contents)
            .unwrap_or_else(|e| die(&format!("invalid JSON in {path}"), e))
    } else {
        Value::Null
    };

    let pipeline = if pipeline_path == "-" {
        let mut buf = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
            die("cannot read stdin", e);
        }
        crux_script::load(&buf).unwrap_or_else(|e| die("cannot parse pipeline from stdin", e))
    } else {
        crux_script::load_file(pipeline_path)
            .unwrap_or_else(|e| die(&format!("cannot load {pipeline_path}"), e))
    };

    warn_missing_env(&pipeline);

    let replay_mode = match replay_mode_str {
        "lenient" => ReplayMode::Lenient,
        _ => ReplayMode::Strict,
    };

    let rt =
        tokio::runtime::Runtime::new().unwrap_or_else(|e| die("cannot start the tokio runtime", e));
    let registry = rt.block_on(build_registry(&pipeline, plugins_path, strict));
    let runner = crux_script::Runner::new(Arc::new(registry));

    let previous: Option<Crux<Value>> = replay_path.map(|path| {
        let contents = std::fs::read_to_string(path)
            .unwrap_or_else(|e| die(&format!("cannot read replay trace {path}"), e));
        serde_json::from_str(&contents)
            .unwrap_or_else(|e| die(&format!("invalid replay trace JSON in {path}"), e))
    });

    let start = Instant::now();
    let crux = if let Some(ref prev) = previous {
        rt.block_on(runner.run_with_replay(&pipeline, input, prev, replay_mode))
    } else {
        rt.block_on(runner.run(&pipeline, input))
    };
    let elapsed = start.elapsed();

    if let Some(path) = save_trace_path {
        let trace_json = serde_json::to_string_pretty(&crux)
            .unwrap_or_else(|e| die("cannot serialize trace", e));
        if let Err(e) = std::fs::write(path, trace_json) {
            die(&format!("cannot write {path}"), e);
        }
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
