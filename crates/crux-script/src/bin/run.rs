/// crux-run: execute a YAML pipeline with stub handlers for demonstration.
///
/// Usage: crux-run <pipeline.yaml> [input.json]
///
/// Without input.json, passes `null` as the pipeline input.
/// Handlers are auto-generated stubs that echo their name and pass data through.
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
    let registry = build_stub_registry(&pipeline);
    let runner = Runner::new(Arc::new(registry));

    let rt = tokio::runtime::Runtime::new().unwrap();
    let start = Instant::now();
    let crux = rt.block_on(runner.run(&pipeline, input));
    let elapsed = start.elapsed();

    print_trace(&crux, elapsed);
}

/// Build a registry with stub handlers for every name referenced in the pipeline.
fn build_stub_registry(pipeline: &PipelineDef) -> HandlerRegistry {
    let mut reg = HandlerRegistry::new();
    let names = collect_handler_names(pipeline);

    for name in names {
        let n = name.clone();
        reg.handler(name, move |input: Value| {
            let handler_name = n.clone();
            async move {
                // Stub: return a JSON object identifying what ran
                Ok(json!({
                    "_handler": handler_name,
                    "_input_type": type_label(&input),
                    "confidence": 0.75,
                    "score": 0.75,
                    "result": "ok"
                }))
            }
        });
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

fn type_label(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
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
