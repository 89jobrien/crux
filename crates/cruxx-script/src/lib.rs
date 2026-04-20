/// cruxx-script: YAML-driven pipeline scripting for the cruxx agentic DSL.
///
/// Define agent pipelines declaratively in YAML files, register step handlers
/// in Rust, and execute without recompilation.
pub mod expr;
pub mod handler_output;
pub mod registry;
pub mod runner;
pub mod schema;

use schema::PipelineDef;

/// Load a pipeline definition from a YAML string.
pub fn load(yaml: &str) -> Result<PipelineDef, serde_saphyr::Error> {
    serde_saphyr::from_str(yaml)
}

/// Load a pipeline definition from a file path.
pub fn load_file(path: impl AsRef<std::path::Path>) -> Result<PipelineDef, LoadError> {
    let contents = std::fs::read_to_string(path)?;
    Ok(serde_saphyr::from_str(&contents)?)
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_saphyr::Error),
}

pub use handler_output::HandlerOutput;
pub use registry::HandlerRegistry;
pub use runner::Runner;
