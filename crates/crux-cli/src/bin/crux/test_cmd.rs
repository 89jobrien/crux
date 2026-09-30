use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct PipelineFixture {
    pipeline: String,
    #[serde(default)]
    input: Value,
    #[serde(default)]
    handlers: BTreeMap<String, Value>,
    expected: Value,
}

pub fn cmd_test(path: &str) {
    if let Err(error) = run_fixture(path) {
        eprintln!("pipeline test failed: {error}");
        std::process::exit(1);
    }
    println!("passed {path}");
}

fn run_fixture(path: &str) -> Result<(), String> {
    let contents = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let fixture: PipelineFixture =
        serde_json::from_str(&contents).map_err(|error| error.to_string())?;
    let pipeline = crux_script::load(&fixture.pipeline).map_err(|error| error.to_string())?;
    let mut registry = crux_script::HandlerRegistry::new();
    for (name, output) in fixture.handlers {
        registry.handler_value(name, move |_input: Value| {
            let output = output.clone();
            async move { Ok(output) }
        });
    }
    let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
    let trace = runtime.block_on(
        crux_script::Runner::new(std::sync::Arc::new(registry)).run(&pipeline, fixture.input),
    );
    let actual = trace.value.map_err(|error| error.to_string())?;
    if actual != fixture.expected {
        return Err(format!("expected {}, got {}", fixture.expected, actual));
    }
    Ok(())
}
