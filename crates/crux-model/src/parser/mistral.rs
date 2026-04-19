use crate::{canonical::CanonicalModelId, error::ModelParseError, vendor::Vendor};

pub fn parse(raw: &str) -> Result<CanonicalModelId, ModelParseError> {
    let parts: Vec<&str> = raw.splitn(3, '-').collect();
    let family = parts[0].to_string();
    let generation = parts.get(1).copied().unwrap_or("").to_string();
    let variant = parts.get(2).copied().unwrap_or("").to_string();

    Ok(CanonicalModelId {
        vendor: Vendor::Mistral,
        family,
        generation,
        variant,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mistral_large_latest() {
        let id = parse("mistral-large-latest").unwrap();
        assert_eq!(id.family, "mistral");
        assert_eq!(id.generation, "large");
        assert_eq!(id.variant, "latest");
    }

    #[test]
    fn codestral_latest() {
        let id = parse("codestral-latest").unwrap();
        assert_eq!(id.family, "codestral");
        assert_eq!(id.generation, "latest");
        assert_eq!(id.variant, "");
    }

    #[test]
    fn single_word() {
        let id = parse("mistral").unwrap();
        assert_eq!(id.family, "mistral");
        assert_eq!(id.generation, "");
        assert_eq!(id.variant, "");
    }
}
