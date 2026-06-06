use crux_script::schema::PipelineDef;

use crate::registry::{build_registry, collect_handler_names};

pub fn cmd_check(paths: &[String]) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut parse_errors = 0u32;
    let mut errors = 0u32;
    let mut warnings = 0u32;

    // Build a registry once with all built-in handlers for validation.
    let empty_pipeline = PipelineDef {
        pipeline: String::new(),
        budget: None,
        steps: vec![],
    };
    let registry = rt.block_on(build_registry(&empty_pipeline, None, false));

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
