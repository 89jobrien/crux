/// crux-run: execute a YAML pipeline using crux-agentic built-in handlers.
///
/// Usage: crux-run <pipeline.yaml> [input.json]
///
/// Without input.json, passes `null` as the pipeline input.
/// Known handlers come from `crux_agentic::register_all`; unknown names degrade
/// to stubs that emit a warning and return a minimal JSON object.
use std::sync::Arc;
use std::time::Instant;

use cruxai_core::prelude::*;
use cruxai_script::{HandlerRegistry, Runner, schema::PipelineDef};
use serde_json::{Value, json};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: crux-run <pipeline.yaml> [input.json]");
        std::process::exit(1);
    }

    let pipeline_path = &args[1];
    let input: Value = if args.len() >= 3 {
        let contents = std::fs::read_to_string(&args[2]).expect("failed to read input file");
        serde_json::from_str(&contents).expect("invalid JSON input")
    } else {
        Value::Null
    };

    let pipeline = cruxai_script::load_file(pipeline_path).expect("failed to load pipeline");
    let registry = build_registry(&pipeline);
    let runner = Runner::new(Arc::new(registry));

    let rt = tokio::runtime::Runtime::new().unwrap();
    let start = Instant::now();
    let crux = rt.block_on(runner.run(&pipeline, input));
    let elapsed = start.elapsed();

    print_trace(&crux, elapsed);
}

/// Build a registry seeded with all crux-agentic built-in handlers.
///
/// Any handler name referenced in the pipeline that has no built-in implementation
/// degrades to a stub that emits a warning and returns a minimal JSON object.
fn build_registry(pipeline: &PipelineDef) -> HandlerRegistry {
    let mut reg = HandlerRegistry::new();
    crux_agentic::register_all(&mut reg);

    // Degrade unknown names to stubs with a warning.
    for name in collect_handler_names(pipeline) {
        if reg.get_handler(&name).is_none() {
            let n = name.clone();
            reg.handler(name, move |_input: Value| {
                let handler_name = n.clone();
                async move {
                    eprintln!(
                        "[crux-run] warning: no builtin for '{handler_name}', using stub"
                    );
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
    use cruxai_script::schema::StepDef;
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
                names.extend(node.stages.clone());
            }
            StepDef::JoinAll(node) => {
                names.extend(node.arms.clone());
            }
            StepDef::RouteOnConfidence(node) => {
                for route in &node.routes {
                    names.push(route.handler.clone());
                }
            }
            StepDef::Speculate(node) => {
                names.extend(node.arms.clone());
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
