use crate::{canonical::CanonicalModelId, error::ModelParseError, vendor::Vendor};

pub fn parse(raw: &str) -> Result<CanonicalModelId, ModelParseError> {
    let Some(rest) = raw.strip_prefix("gemini-") else {
        return Ok(super::fallback::parse(Vendor::Google, raw));
    };

    // First segment is generation (e.g. "2.5"), rest is variant
    let (generation, variant) = match rest.find('-') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };

    Ok(CanonicalModelId {
        vendor: Vendor::Google,
        family: "gemini".to_string(),
        generation: generation.to_string(),
        variant: variant.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_2_5_pro() {
        let id = parse("gemini-2.5-pro").unwrap();
        assert_eq!(id.family, "gemini");
        assert_eq!(id.generation, "2.5");
        assert_eq!(id.variant, "pro");
    }

    #[test]
    fn gemini_1_5_flash() {
        let id = parse("gemini-1.5-flash-8b").unwrap();
        assert_eq!(id.family, "gemini");
        assert_eq!(id.generation, "1.5");
        assert_eq!(id.variant, "flash-8b");
    }

    #[test]
    fn unknown_falls_back() {
        let id = parse("palm-2").unwrap();
        assert_eq!(id.family, "palm-2");
    }
}
