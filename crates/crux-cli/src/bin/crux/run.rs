use std::io::Read as _;
use std::sync::Arc;
use std::time::Instant;

use crux_runtime::prelude::*;
use crux_script::{HandlerRegistry, TargetResolver, schema::PipelineDef};
use serde_json::{Value, json};

use crate::registry::{build_registry, collect_handler_names, render_trace, warn_missing_env};

/// Render the default (non-verbose) `crux run` output: raw JSON of the result value.
///
/// Pure: no I/O. On success, returns the compact JSON encoding of the value. On
/// failure, returns the error message (printed to stderr by the caller).
fn render_default_output(crux: &Crux<Value>) -> Result<String, String> {
    match crux.value() {
        Ok(v) => Ok(serde_json::to_string(v).unwrap_or_default()),
        Err(e) => Err(e.to_string()),
    }
}

/// Shared config for the `run` subcommand, replacing positional arg sprawl.
pub struct RunConfig<'a> {
    pub pipeline_arg: Option<&'a str>,
    pub target_or_input: Option<&'a str>,
    pub check: bool,
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
        let target_name = select_target_name(cfg).map(String::from);
        if cfg.dry_run {
            cmd_dry_run_cruxfile(contents, pipeline_path, target_name.as_deref());
        } else {
            cmd_run_cruxfile(contents, pipeline_path, target_name.as_deref(), cfg);
        }
    } else {
        // Regular pipeline. target_or_input is the input file, not a target.
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
        if cfg.check {
            eprintln!("error: --check does not support stdin ('-') pipelines");
            std::process::exit(1);
        }
        // stdin path — always a regular pipeline
        cmd_run("-", cfg.target_or_input.or(cfg.input_flag), cfg);
        return;
    };

    if cfg.check {
        crate::check::cmd_check(&[pipeline_path]);
        return;
    }

    let contents = std::fs::read_to_string(&pipeline_path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {pipeline_path}: {e}");
        std::process::exit(1);
    });

    dispatch_on_contents(&contents, &pipeline_path, cfg);
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
                vars: None,
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
fn cmd_run_cruxfile(contents: &str, path: &str, target_name: Option<&str>, cfg: &RunConfig<'_>) {
    let plugins_path = cfg.plugins_path;
    let quiet = cfg.quiet;
    let verbose = cfg.verbose;
    let save_trace_path = cfg.save_trace_path;
    let strict = cfg.strict;
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
        vars: None,
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
            vars: None,
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

fn cmd_run(pipeline_path: &str, input_path: Option<&str>, cfg: &RunConfig<'_>) {
    let plugins_path = cfg.plugins_path;
    let quiet = cfg.quiet;
    let verbose = cfg.verbose;
    let replay_path = cfg.replay_path;
    let replay_mode_str = cfg.replay_mode_str;
    let save_trace_path = cfg.save_trace_path;
    let strict = cfg.strict;
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
    let registry = rt.block_on(build_registry(&pipeline, plugins_path, strict));
    let runner = crux_script::Runner::new(Arc::new(registry));

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
        print!("{}", render_trace(&crux, elapsed));
        if crux.value().is_err() {
            std::process::exit(1);
        }
    } else if !quiet {
        match render_default_output(&crux) {
            Ok(json) => println!("{json}"),
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

fn register_stub_handler(reg: &mut HandlerRegistry, name: String) {
    let n = name.clone();
    reg.handler_value(name, move |_input: Value| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crux_runtime::prelude::CruxId;

    fn ok_crux(v: Value) -> Crux<Value> {
        Crux {
            id: CruxId::new(),
            agent: "test-agent".to_string(),
            value: Ok(v),
            steps: vec![],
            children: vec![],
            started_at: chrono::Utc::now(),
            finished_at: None,
        }
    }

    #[test]
    fn default_output_is_raw_json_of_result() {
        let crux = ok_crux(json!({"answer": 42}));
        let out = render_default_output(&crux).expect("ok result");
        assert_eq!(out, r#"{"answer":42}"#);
        // No trace envelope framing should leak into the default output.
        assert!(!out.contains("Pipeline:"));
        assert!(!out.contains("Trace:"));
    }

    #[test]
    fn verbose_output_is_full_trace_envelope() {
        let crux = ok_crux(json!({"answer": 42}));
        let out = render_trace(&crux, std::time::Duration::from_millis(5));
        assert!(out.contains("Pipeline:"));
        assert!(out.contains("Status:   OK"));
        assert!(out.contains("Trace:"));
        assert!(out.contains("Output:"));
        assert!(out.contains(r#""answer": 42"#));
    }
}
