//! crux-model: canonical model ID types and provider-specific parsers.
//!
//! Normalizes LLM model identifiers across providers (OpenAI, Anthropic,
//! Google, etc.) into a canonical form for consistent routing and billing.

pub mod canonical;
pub mod error;
pub mod parser;
pub mod provider_ref;
pub mod vendor;

pub use canonical::CanonicalModelId;
pub use error::ModelParseError;
pub use parser::ProviderModelId;
pub use provider_ref::{ModelMetadata, ProviderModelRef};
pub use vendor::Vendor;
