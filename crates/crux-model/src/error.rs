use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelParseError {
    #[error("unknown vendor: '{0}'")]
    UnknownVendor(String),
    #[error("invalid canonical model ID format: '{0}' (expected vendor:family:generation:variant)")]
    InvalidFormat(String),
}
