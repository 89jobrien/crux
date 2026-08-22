//! crux-script: YAML-driven pipeline scripting for the crux agentic DSL.
//!
//! Define agent pipelines declaratively in YAML files, register step handlers
//! in Rust, and execute without recompilation.
// TODO(#99): pipeline validation pass — catch bad refs, missing handlers, type
//   mismatches, and unreachable steps before execution starts (static analysis)
pub mod expr;
pub mod handler_output;
pub mod metadata;
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

pub use handler_output::HandlerOutput;
pub use metadata::{
    ArgSchema, ArgSpec, ArgType, Capability, HandlerMetadata, RiskLevel, SideEffect,
};
pub use registry::HandlerRegistry;
pub use resolve::{ResolveError, TargetResolver};
pub use runner::Runner;
pub use step_runner::{RunnerCapability, StepContext, StepOutput, StepRunner, StepRunnerRegistry};
pub use validator::{
    DiagnosticSeverity, ValidationDiagnostic, ValidationReport, validate_cruxfile,
    validate_pipeline,
};
