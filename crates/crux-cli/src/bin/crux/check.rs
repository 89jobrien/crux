use crux_script::{
    Compilation, CompileOptions, DiagnosticSeverity, ValidationDiagnostic, compile_cruxfile,
    compile_pipeline,
};

use crate::registry::{build_base_registry, collect_handler_names};

/// Compatibility entry point used by `crux run --check`.
pub fn cmd_check(paths: &[String]) {
    cmd_check_with_options(paths, None, false);
}

/// Compile-check pipeline files with the same registry used for execution.
pub fn cmd_check_with_options(paths: &[String], plugins_path: Option<&str>, strict: bool) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut parse_errors = 0usize;
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let options = if strict {
        CompileOptions::strict()
    } else {
        CompileOptions::permissive()
    };
    let registry = rt.block_on(build_base_registry(plugins_path));

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

            let target_count = cruxfile.targets.len();
            let compilation = compile_cruxfile(&cruxfile, &registry, options);
            render_compilation(
                path,
                &compilation,
                &format!(
                    "Cruxfile, {target_count} targets, default: {}",
                    cruxfile.default
                ),
                &mut errors,
                &mut warnings,
            );
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

        let step_count = pipeline.steps.len();
        let handlers = collect_handler_names(&pipeline);
        let compilation = compile_pipeline(&pipeline, &registry, options);
        render_compilation(
            path,
            &compilation,
            &format!("{step_count} steps, handlers: {}", handlers.join(", ")),
            &mut errors,
            &mut warnings,
        );
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

fn render_compilation<T>(
    path: &str,
    compilation: &Compilation<T>,
    description: &str,
    errors: &mut usize,
    warnings: &mut usize,
) {
    for diagnostic in compilation.diagnostics() {
        render_diagnostic(path, diagnostic);
        match diagnostic.severity {
            DiagnosticSeverity::Error => *errors += 1,
            DiagnosticSeverity::Warning => *warnings += 1,
        }
    }

    if compilation.diagnostics().is_empty() {
        println!("\x1b[32mok\x1b[0m: {path} ({description})");
    } else if compilation.is_ok() && !compilation.is_executable() {
        eprintln!("\x1b[33mwarning\x1b[0m: {path}: check passed, but pipeline is not executable");
    }
}

fn render_diagnostic(path: &str, diagnostic: &ValidationDiagnostic) {
    let color = match diagnostic.severity {
        DiagnosticSeverity::Error => "\x1b[31m",
        DiagnosticSeverity::Warning => "\x1b[33m",
    };
    eprintln!(
        "{color}{}[{}]\x1b[0m: {path} [{}]: {}",
        diagnostic.severity, diagnostic.code, diagnostic.location, diagnostic.message
    );
}
