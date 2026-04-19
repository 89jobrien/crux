use crate::{canonical::CanonicalModelId, error::ModelParseError, vendor::Vendor};

pub fn parse(raw: &str) -> Result<CanonicalModelId, ModelParseError> {
    Ok(super::fallback::parse(Vendor::Ollama, raw))
}
