use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelParseError {
    UnknownVendor(String),
    InvalidFormat(String),
    MissingSegment { position: usize },
}

impl fmt::Display for ModelParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownVendor(v) => write!(f, "unknown vendor: '{v}'"),
            Self::InvalidFormat(s) => write!(
                f,
                "invalid canonical model ID format: '{s}' (expected vendor:family:generation:variant)"
            ),
            Self::MissingSegment { position } => {
                write!(f, "missing segment at position {position}")
            }
        }
    }
}

impl std::error::Error for ModelParseError {}
