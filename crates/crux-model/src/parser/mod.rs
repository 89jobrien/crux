pub mod anthropic;
pub mod fallback;
pub mod google;
pub mod mistral;
pub mod ollama;
pub mod openai;

use crate::{error::ModelParseError, provider_ref::ProviderModelRef, vendor::Vendor};

pub struct ProviderModelId;

impl ProviderModelId {
    pub fn parse(vendor: Vendor, raw: &str) -> Result<ProviderModelRef, ModelParseError> {
        let canonical = match vendor {
            Vendor::Anthropic => anthropic::parse(raw)?,
            Vendor::OpenAi => openai::parse(raw)?,
            Vendor::Google => google::parse(raw)?,
            Vendor::Mistral => mistral::parse(raw)?,
            Vendor::Ollama => ollama::parse(raw)?,
            _ => fallback::parse(vendor, raw),
        };
        Ok(ProviderModelRef::new(vendor, raw, canonical))
    }

    pub fn parse_lenient(vendor: Vendor, raw: &str) -> ProviderModelRef {
        let canonical = fallback::parse(vendor, raw);
        ProviderModelRef::new(vendor, raw, canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lenient_never_fails() {
        let r = ProviderModelId::parse_lenient(Vendor::Anthropic, "some-weird-model-string");
        assert_eq!(r.vendor, Vendor::Anthropic);
        assert_eq!(r.provider_id, "some-weird-model-string");
    }

    #[test]
    fn parse_lenient_preserves_provider_id() {
        let raw = "gpt-4o-2024-11-20";
        let r = ProviderModelId::parse_lenient(Vendor::OpenAi, raw);
        assert_eq!(r.provider_id, raw);
        assert_eq!(r.vendor, Vendor::OpenAi);
    }
}
