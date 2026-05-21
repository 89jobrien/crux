use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::error::ModelParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    OpenAi,
    Anthropic,
    Google,
    Meta,
    Mistral,
    Cohere,
    Ollama,
    Local,
}

impl fmt::Display for Vendor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::Google => "google",
            Self::Meta => "meta",
            Self::Mistral => "mistral",
            Self::Cohere => "cohere",
            Self::Ollama => "ollama",
            Self::Local => "local",
        };
        f.write_str(s)
    }
}

impl FromStr for Vendor {
    type Err = ModelParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(Self::OpenAi),
            "anthropic" => Ok(Self::Anthropic),
            "google" => Ok(Self::Google),
            "meta" => Ok(Self::Meta),
            "mistral" => Ok(Self::Mistral),
            "cohere" => Ok(Self::Cohere),
            "ollama" => Ok(Self::Ollama),
            "local" => Ok(Self::Local),
            other => Err(ModelParseError::UnknownVendor(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_lowercase() {
        assert_eq!(Vendor::OpenAi.to_string(), "openai");
        assert_eq!(Vendor::Anthropic.to_string(), "anthropic");
        assert_eq!(Vendor::Google.to_string(), "google");
        assert_eq!(Vendor::Meta.to_string(), "meta");
        assert_eq!(Vendor::Mistral.to_string(), "mistral");
        assert_eq!(Vendor::Cohere.to_string(), "cohere");
        assert_eq!(Vendor::Ollama.to_string(), "ollama");
        assert_eq!(Vendor::Local.to_string(), "local");
    }

    #[test]
    fn from_str_case_insensitive() {
        assert_eq!("OpenAI".parse::<Vendor>().unwrap(), Vendor::OpenAi);
        assert_eq!("ANTHROPIC".parse::<Vendor>().unwrap(), Vendor::Anthropic);
        assert_eq!("Google".parse::<Vendor>().unwrap(), Vendor::Google);
    }

    #[test]
    fn from_str_unknown_errors() {
        let err = "bogus".parse::<Vendor>().unwrap_err();
        assert_eq!(err, ModelParseError::UnknownVendor("bogus".to_string()));
    }

    #[test]
    fn serde_roundtrip() {
        let v = Vendor::Mistral;
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, r#""mistral""#);
        let back: Vendor = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v);
    }
}
