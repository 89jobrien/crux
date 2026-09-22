//! crux-model: canonical model ID types and provider-specific parsers.
//!
//! Normalizes LLM model identifiers across providers (OpenAI, Anthropic,
//! Google, etc.) into a canonical form for consistent routing and billing.

pub mod canonical;
pub mod error;
pub mod parser;
pub mod provider_ref;
pub mod registry;
pub mod vendor;

pub use canonical::CanonicalModelId;
pub use error::ModelParseError;
pub use parser::ProviderModelId;
pub use provider_ref::{ModelMetadata, ProviderModelRef};
pub use registry::{ModelCapabilities, ModelCost, ModelRegistry, ModelRequirements};
pub use vendor::Vendor;

#[cfg(test)]
mod capability_registry_tests {
    use super::*;

    #[test]
    fn registry_selects_models_by_required_capabilities() {
        let mut registry = ModelRegistry::new();
        let id: CanonicalModelId = "anthropic:claude:4:sonnet".parse().unwrap();
        registry.register(
            id.clone(),
            ModelCapabilities {
                context_window: 200_000,
                supports_tools: true,
                supports_json_mode: true,
                supports_vision: true,
                cost: Some(ModelCost {
                    input_cents_per_million_tokens: 300,
                    output_cents_per_million_tokens: 1_500,
                }),
                aliases: vec!["claude-sonnet-4".into()],
                deprecated: false,
            },
        );

        let requirements = ModelRequirements {
            min_context_window: 128_000,
            requires_tools: true,
            requires_json_mode: true,
            requires_vision: false,
            allow_deprecated: false,
        };

        assert_eq!(registry.select(&requirements), vec![id.clone()]);
        assert_eq!(registry.resolve("claude-sonnet-4"), Some(&id));
    }
}
