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

use schema::{CruxfileDef, PipelineDef};

/// Load a pipeline definition from a YAML string.
pub fn load(yaml: &str) -> Result<PipelineDef, serde_saphyr::Error> {
    serde_saphyr::from_str(yaml)
}

/// Load a pipeline definition from a file path.
pub fn load_file(path: impl AsRef<std::path::Path>) -> Result<PipelineDef, LoadError> {
    let mut stack = Vec::new();
    let value = load_composed_value(path.as_ref(), &mut stack)?;
    let contents = serde_yaml::to_string(&value)?;
    Ok(serde_saphyr::from_str(&contents)?)
}

fn load_composed_value(
    path: &std::path::Path,
    stack: &mut Vec<std::path::PathBuf>,
) -> Result<serde_yaml::Value, LoadError> {
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
    let mut value: serde_yaml::Value = serde_yaml::from_str(&contents)?;
    let base = canonical
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    expand_includes(&mut value, base, stack)?;
    stack.pop();
    Ok(value)
}

fn expand_includes(
    value: &mut serde_yaml::Value,
    base: &std::path::Path,
    stack: &mut Vec<std::path::PathBuf>,
) -> Result<(), LoadError> {
    match value {
        serde_yaml::Value::Mapping(mapping) => {
            if let Some(serde_yaml::Value::Sequence(steps)) =
                mapping.get_mut(serde_yaml::Value::String("steps".to_string()))
            {
                let mut expanded = Vec::new();
                for mut step in std::mem::take(steps) {
                    let include = step.as_mapping().and_then(|entry| {
                        ["include", "import"].into_iter().find_map(|key| {
                            entry
                                .get(serde_yaml::Value::String(key.to_string()))
                                .and_then(serde_yaml::Value::as_str)
                        })
                    });
                    if let Some(include) = include {
                        let child = load_composed_value(&base.join(include), stack)?;
                        let child_steps = child
                            .as_mapping()
                            .and_then(|pipeline| {
                                pipeline.get(serde_yaml::Value::String("steps".to_string()))
                            })
                            .and_then(serde_yaml::Value::as_sequence)
                            .ok_or_else(|| LoadError::InvalidInclude(include.to_string()))?;
                        expanded.extend(child_steps.iter().cloned());
                    } else {
                        expand_includes(&mut step, base, stack)?;
                        expanded.push(step);
                    }
                }
                *steps = expanded;
            }
            for nested in mapping.values_mut() {
                expand_includes(nested, base, stack)?;
            }
        }
        serde_yaml::Value::Sequence(values) => {
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
    CompositionYaml(#[from] serde_yaml::Error),
    #[error("include cycle detected: {0}")]
    IncludeCycle(String),
    #[error("included pipeline '{0}' has no steps sequence")]
    InvalidInclude(String),
}

pub use compiler::{Compilation, CompileMode, CompileOptions, compile_cruxfile, compile_pipeline};
pub use handler_output::{HandlerExecution, HandlerOutput};
pub use ir::{TypedCruxfile, TypedPipeline};
pub use metadata::{
    AgentMetadata, ArgSchema, ArgSpec, ArgType, Capability, ConfidenceCapability, HandlerMetadata,
    ObjectSchema, RiskLevel, SchemaBuildError, SchemaProperty, SchemaViolation,
    SchemaViolationKind, SideEffect, ValueKind, ValueSchema,
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
