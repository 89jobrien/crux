use serde::{Deserialize, Serialize};

use crate::{canonical::CanonicalModelId, vendor::Vendor};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderModelRef {
    pub vendor: Vendor,
    pub provider_id: String,
    pub canonical: CanonicalModelId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ModelMetadata>,
}

impl ProviderModelRef {
    pub fn new(
        vendor: Vendor,
        provider_id: impl Into<String>,
        canonical: CanonicalModelId,
    ) -> Self {
        Self {
            vendor,
            provider_id: provider_id.into(),
            canonical,
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, meta: ModelMetadata) -> Self {
        self.metadata = Some(meta);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vendor::Vendor;

    fn example_canonical() -> CanonicalModelId {
        CanonicalModelId {
            vendor: Vendor::Anthropic,
            family: "claude".to_string(),
            generation: "3-5".to_string(),
            variant: "sonnet".to_string(),
        }
    }

    #[test]
    fn new_creates_ref_without_metadata() {
        let r = ProviderModelRef::new(
            Vendor::Anthropic,
            "claude-3-5-sonnet-20241022",
            example_canonical(),
        );
        assert_eq!(r.vendor, Vendor::Anthropic);
        assert_eq!(r.provider_id, "claude-3-5-sonnet-20241022");
        assert!(r.metadata.is_none());
    }

    #[test]
    fn with_metadata_attaches() {
        let meta = ModelMetadata {
            family: Some("claude".to_string()),
            parameter_size: None,
            quantization: None,
            format: None,
        };
        let r = ProviderModelRef::new(
            Vendor::Anthropic,
            "claude-3-5-sonnet-20241022",
            example_canonical(),
        )
        .with_metadata(meta.clone());
        assert_eq!(r.metadata, Some(meta));
    }

    #[test]
    fn serde_roundtrip() {
        let r = ProviderModelRef::new(
            Vendor::Anthropic,
            "claude-3-5-sonnet-20241022",
            example_canonical(),
        );
        let json = serde_json::to_string(&r).unwrap();
        let back: ProviderModelRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }
}
