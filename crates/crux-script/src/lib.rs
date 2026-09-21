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
    let contents = std::fs::read_to_string(path)?;
    Ok(serde_saphyr::from_str(&contents)?)
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
