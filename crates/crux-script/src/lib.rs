//! crux-script: YAML-driven pipeline scripting for the crux agentic DSL.
//!
//! Define agent pipelines declaratively in YAML files, register step handlers
//! in Rust, and execute without recompilation.
pub mod compiler;
pub mod expr;
pub mod handler_output;
pub mod ir;
pub mod metadata;
pub mod output;
pub mod registry;
pub mod resolve;
pub mod runner;
pub mod schema;
pub mod step_runner;
pub mod validator;

pub use schema::{CostTierDef, ModelRequirementsDef};
use schema::{CruxfileDef, PipelineDef};
use serde_json::Value as JsonValue;

/// Load a pipeline definition from a YAML string.
pub fn load(yaml: &str) -> Result<PipelineDef, serde_saphyr::Error> {
    serde_saphyr::from_str(yaml)
}

/// Load a pipeline definition from a file path.
pub fn load_file(path: impl AsRef<std::path::Path>) -> Result<PipelineDef, LoadError> {
    let mut stack = Vec::new();
    let value = load_composed_value(path.as_ref(), &mut stack)?;
    let contents = serde_saphyr::to_string(&value)?;
    Ok(serde_saphyr::from_str(&contents)?)
}

fn load_composed_value(
    path: &std::path::Path,
    stack: &mut Vec<std::path::PathBuf>,
) -> Result<JsonValue, LoadError> {
    let canonical = path.canonicalize()?;
    if let Some(start) = stack.iter().position(|ancestor| ancestor == &canonical) {
        let mut cycle = stack[start..].to_vec();
        cycle.push(canonical);
        return Err(LoadError::IncludeCycle(
            cycle
                .iter()
                .map(|entry| entry.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> "),
        ));
    }
    stack.push(canonical.clone());
    let contents = std::fs::read_to_string(&canonical)?;
    // Composition walks a JSON-shaped tree: `serde-saphyr` exposes `from_str` and
    // `to_string` but no YAML value type, and pipeline YAML is JSON-representable
    // by construction (string keys, standard scalars).
    let mut value: JsonValue = serde_saphyr::from_str(&contents)?;
    let base = canonical
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    expand_includes(&mut value, base, stack)?;
    stack.pop();
    Ok(value)
}

/// Returns the `include`/`import` target of a step entry, if it declares one.
fn include_target(step: &JsonValue) -> Option<&str> {
    let entry = step.as_object()?;
    ["include", "import"]
        .into_iter()
        .find_map(|key| entry.get(key).and_then(JsonValue::as_str))
}

fn expand_includes(
    value: &mut JsonValue,
    base: &std::path::Path,
    stack: &mut Vec<std::path::PathBuf>,
) -> Result<(), LoadError> {
    match value {
        JsonValue::Object(mapping) => {
            if let Some(JsonValue::Array(steps)) = mapping.get_mut("steps") {
                let mut expanded = Vec::new();
                for mut step in std::mem::take(steps) {
                    let include = include_target(&step).map(str::to_string);
                    match include {
                        Some(include) => {
                            let child = load_composed_value(&base.join(&include), stack)?;
                            let child_steps = child
                                .get("steps")
                                .and_then(JsonValue::as_array)
                                .ok_or_else(|| LoadError::InvalidInclude(include.clone()))?;
                            expanded.extend(child_steps.iter().cloned());
                        }
                        None => {
                            expand_includes(&mut step, base, stack)?;
                            expanded.push(step);
                        }
                    }
                }
                *steps = expanded;
            }
            for nested in mapping.values_mut() {
                expand_includes(nested, base, stack)?;
            }
        }
        JsonValue::Array(values) => {
            for nested in values {
                expand_includes(nested, base, stack)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Generate the JSON Schema used to validate declarative pipeline files.
pub fn pipeline_json_schema() -> schemars::Schema {
    schemars::schema_for!(PipelineDef)
}

/// Detect whether a YAML string is a Cruxfile (multi-target) rather than a pipeline.
pub fn is_cruxfile(yaml: &str) -> bool {
    // Quick heuristic: Cruxfile has `targets:` key, pipelines have `pipeline:`.
    yaml.lines()
        .any(|line| line.starts_with("targets:") || line.starts_with("targets :"))
}

/// Load a Cruxfile definition from a YAML string.
pub fn load_cruxfile(yaml: &str) -> Result<CruxfileDef, serde_saphyr::Error> {
    serde_saphyr::from_str(yaml)
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_saphyr::Error),
    #[error("YAML composition error: {0}")]
    CompositionSerialize(#[from] serde_saphyr::SerializeError),
    #[error("include cycle detected: {0}")]
    IncludeCycle(String),
    #[error("included pipeline '{0}' has no steps sequence")]
    InvalidInclude(String),
}

pub use compiler::{Compilation, CompileMode, CompileOptions, compile_cruxfile, compile_pipeline};
pub use handler_output::{HandlerExecution, HandlerOutput};
pub use ir::{TypedCruxfile, TypedPipeline};
pub use metadata::{
    AgentMetadata, ArgSchema, ArgSpec, ArgType, Capability, CapabilityViolation,
    ConfidenceCapability, HandlerMetadata, ObjectSchema, RiskLevel, SchemaBuildError,
    SchemaProperty, SchemaViolation, SchemaViolationKind, SideEffect, ValueKind, ValueSchema,
};
pub use output::{
    PipelineOutputError, PipelineOutputFormat, collect_agent_names, collect_handler_names,
    format_pipeline_output,
};
pub use registry::{HandlerRegistry, RegistryError};
pub use resolve::{ResolveError, TargetResolver};
pub use runner::Runner;
pub use step_runner::{
    RunnerCapability, StepContext, StepFuture, StepInvocation, StepOutput, StepRunner,
    StepRunnerRegistry,
};
pub use validator::{
    DiagnosticSeverity, ValidationCode, ValidationDiagnostic, ValidationReport, validate_cruxfile,
    validate_pipeline,
};
