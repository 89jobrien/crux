use cruxx_core::prelude::CruxErr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgenticError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing arg: {0}")]
    MissingArg(&'static str),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Other(String),
}

impl From<AgenticError> for CruxErr {
    fn from(e: AgenticError) -> Self {
        CruxErr::step_failed("agentic", e.to_string())
    }
}

/// Convenience: extract a string arg from handler input JSON.
pub fn require_str<'a>(
    input: &'a serde_json::Value,
    key: &'static str,
) -> Result<&'a str, AgenticError> {
    input
        .get("args")
        .and_then(|a| a.get(key))
        .and_then(|v| v.as_str())
        .ok_or(AgenticError::MissingArg(key))
}

/// Convenience: extract an optional string arg.
pub fn opt_str<'a>(input: &'a serde_json::Value, key: &'static str) -> Option<&'a str> {
    input
        .get("args")
        .and_then(|a| a.get(key))
        .and_then(|v| v.as_str())
}
