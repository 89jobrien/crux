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
        match Self::parse(vendor, raw) {
            Ok(r) => r,
            Err(_) => {
                let canonical = fallback::parse(vendor, raw);
                ProviderModelRef::new(vendor, raw, canonical)
            }
        }
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

#[cfg(test)]
mod proptest_roundtrip {
    use proptest::prelude::*;

    use super::*;
    use crate::canonical::CanonicalModelId;

    /// Strategy for generating ASCII model ID strings that are safe to feed
    /// into vendor parsers without triggering panics.
    fn model_id_strategy() -> impl Strategy<Value = String> {
        "[a-z0-9][a-z0-9._-]{0,30}"
    }

    fn all_vendors() -> Vec<Vendor> {
        vec![
            Vendor::Anthropic,
            Vendor::OpenAi,
            Vendor::Google,
            Vendor::Mistral,
            Vendor::Ollama,
        ]
    }

    proptest! {
        /// `parse_lenient` must never panic for any vendor and any ASCII input.
        #[test]
        fn parse_lenient_does_not_panic(raw in model_id_strategy()) {
            for vendor in all_vendors() {
                let _ = ProviderModelId::parse_lenient(vendor, &raw);
            }
        }

        /// The canonical ID produced by `parse_lenient` must round-trip through
        /// `Display` → `FromStr` without loss.
        #[test]
        fn canonical_display_fromstr_roundtrip(raw in model_id_strategy()) {
            for vendor in all_vendors() {
                let model_ref = ProviderModelId::parse_lenient(vendor, &raw);
                let canonical = &model_ref.provider_id;
                // Display then re-parse the CanonicalModelId (not the provider ID).
                let canonical_id = &model_ref.canonical;
                let displayed = canonical_id.to_string();
                let reparsed: Result<CanonicalModelId, _> = displayed.parse();
                prop_assert!(
                    reparsed.is_ok(),
                    "canonical roundtrip failed for vendor={vendor:?} raw={canonical}: {displayed}"
                );
                prop_assert_eq!(reparsed.unwrap(), canonical_id.clone());
            }
        }
    }
}
