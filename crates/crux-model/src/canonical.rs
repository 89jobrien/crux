use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{error::ModelParseError, vendor::Vendor};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanonicalModelId {
    pub vendor: Vendor,
    pub family: String,
    pub generation: String,
    pub variant: String,
}

impl CanonicalModelId {
    pub fn as_key(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for CanonicalModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}:{}",
            self.vendor, self.family, self.generation, self.variant
        )
    }
}

impl FromStr for CanonicalModelId {
    type Err = ModelParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const FIELD_COUNT: usize = 4;
        let parts: Vec<&str> = s.splitn(FIELD_COUNT + 1, ':').collect();
        if parts.len() != FIELD_COUNT {
            return Err(ModelParseError::InvalidFormat(s.to_string()));
        }
        let vendor = parts[0].parse::<Vendor>()?;
        Ok(Self {
            vendor,
            family: parts[1].to_string(),
            generation: parts[2].to_string(),
            variant: parts[3].to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> CanonicalModelId {
        CanonicalModelId {
            vendor: Vendor::Anthropic,
            family: "claude".to_string(),
            generation: "3-5".to_string(),
            variant: "sonnet".to_string(),
        }
    }

    #[test]
    fn display_colon_separated() {
        assert_eq!(example().to_string(), "anthropic:claude:3-5:sonnet");
    }

    #[test]
    fn from_str_roundtrip() {
        let id = example();
        let s = id.to_string();
        let back: CanonicalModelId = s.parse().unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn from_str_wrong_segment_count() {
        assert!("anthropic:claude:3-5".parse::<CanonicalModelId>().is_err());
        assert!(
            "anthropic:claude:3-5:sonnet:extra"
                .parse::<CanonicalModelId>()
                .is_err()
        );
    }

    #[test]
    fn from_str_unknown_vendor() {
        let err = "bogus:claude:3-5:sonnet"
            .parse::<CanonicalModelId>()
            .unwrap_err();
        assert!(matches!(err, ModelParseError::UnknownVendor(_)));
    }

    #[test]
    fn from_str_empty_segments_allowed() {
        let id: CanonicalModelId = "openai:::".parse().unwrap();
        assert_eq!(id.vendor, Vendor::OpenAi);
        assert_eq!(id.family, "");
        assert_eq!(id.generation, "");
        assert_eq!(id.variant, "");
    }

    #[test]
    fn serde_struct_form() {
        let id = example();
        let json = serde_json::to_string(&id).unwrap();
        let back: CanonicalModelId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn as_key_matches_display() {
        let id = example();
        assert_eq!(id.as_key(), id.to_string());
    }
}
