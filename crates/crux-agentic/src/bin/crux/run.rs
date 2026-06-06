use std::io::Read as _;
use std::sync::Arc;
use std::time::Instant;

use crux_runtime::prelude::*;
use crux_script::{HandlerRegistry, TargetResolver, schema::PipelineDef};
use serde_json::{Value, json};

use crate::registry::{build_registry, collect_handler_names, print_trace, warn_missing_env};

/// Dispatch between Cruxfile (multi-target) and regular pipeline execution.
#[allow(clippy::too_many_arguments)]
pub fn cmd_run_dispatch(
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
    strict: bool,
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
                strict,
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
                strict,
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
                strict,
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
#[allow(clippy::too_many_arguments)]
fn cmd_run_cruxfile(
    contents: &str,
    path: &str,
    target_name: Option<&str>,
    plugins_path: Option<&str>,
    quiet: bool,
    verbose: bool,
    save_trace_path: Option<&str>,
    strict: bool,
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
    strict: bool,
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
