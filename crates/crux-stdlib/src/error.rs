use cruxx_core::prelude::CruxErr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StdlibError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing arg: {0}")]
    MissingArg(&'static str),
    #[error("{0}")]
    Other(String),
}

impl From<StdlibError> for CruxErr {
    fn from(e: StdlibError) -> Self {
        CruxErr::step_failed("stdlib", e.to_string())
    }
}

/// Convenience: extract a string arg from handler input JSON.
pub fn require_str<'a>(
    input: &'a serde_json::Value,
    key: &'static str,
) -> Result<&'a str, StdlibError> {
    input
        .get("args")
        .and_then(|a| a.get(key))
        .and_then(|v| v.as_str())
        .ok_or(StdlibError::MissingArg(key))
}

/// Convenience: extract an optional string arg.
pub fn opt_str<'a>(input: &'a serde_json::Value, key: &'static str) -> Option<&'a str> {
    input
        .get("args")
        .and_then(|a| a.get(key))
        .and_then(|v| v.as_str())
}
