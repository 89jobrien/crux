use crate::{canonical::CanonicalModelId, error::ModelParseError, vendor::Vendor};

const TIERS: &[&str] = &["opus", "sonnet", "haiku"];

pub fn parse(raw: &str) -> Result<CanonicalModelId, ModelParseError> {
    let Some(rest) = raw.strip_prefix("claude-") else {
        return Ok(super::fallback::parse(Vendor::Anthropic, raw));
    };

    let parts: Vec<&str> = rest.split('-').collect();

    // Pattern A: claude-{tier}-{major}-{minor}[-{date}...]
    // e.g. claude-sonnet-4-6, claude-opus-4-6, claude-haiku-4-5-20251001
    if parts.len() >= 3 && TIERS.contains(&parts[0]) {
        let tier = parts[0];
        let major = parts[1];
        if major.chars().all(|c| c.is_ascii_digit()) {
            let generation = major.to_string();
            let after_major = parts[2..].join("-");
            let variant = format!("{tier}-{after_major}");
            return Ok(CanonicalModelId {
                vendor: Vendor::Anthropic,
                family: "claude".to_string(),
                generation,
                variant,
            });
        }
    }

    // Pattern B: claude-{major}-{minor}-{tier}[-{date}...]
    // e.g. claude-3-5-haiku-20241022
    if parts.len() >= 3 {
        let major = parts[0];
        let minor = parts[1];
        if major.chars().all(|c| c.is_ascii_digit())
            && minor.chars().all(|c| c.is_ascii_digit())
            && TIERS.contains(&parts[2])
        {
            let generation = format!("{major}.{minor}");
            let variant = parts[2..].join("-");
            return Ok(CanonicalModelId {
                vendor: Vendor::Anthropic,
                family: "claude".to_string(),
                generation,
                variant,
            });
        }
    }

    Ok(super::fallback::parse(Vendor::Anthropic, raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vendor::Vendor;

    #[test]
    fn claude_sonnet_4_6() {
        let id = parse("claude-sonnet-4-6").unwrap();
        assert_eq!(id.vendor, Vendor::Anthropic);
        assert_eq!(id.family, "claude");
        assert_eq!(id.generation, "4");
        assert_eq!(id.variant, "sonnet-6");
    }

    #[test]
    fn claude_3_5_haiku() {
        let id = parse("claude-3-5-haiku-20241022").unwrap();
        assert_eq!(id.family, "claude");
        assert_eq!(id.generation, "3.5");
        assert_eq!(id.variant, "haiku-20241022");
    }

    #[test]
    fn claude_opus_4_6() {
        let id = parse("claude-opus-4-6").unwrap();
        assert_eq!(id.family, "claude");
        assert_eq!(id.generation, "4");
        assert_eq!(id.variant, "opus-6");
    }

    #[test]
    fn claude_haiku_4_5() {
        let id = parse("claude-haiku-4-5-20251001").unwrap();
        assert_eq!(id.family, "claude");
        assert_eq!(id.generation, "4");
        assert_eq!(id.variant, "haiku-5-20251001");
    }

    #[test]
    fn unknown_falls_back() {
        let id = parse("some-new-model").unwrap();
        assert_eq!(id.family, "some-new-model");
        assert_eq!(id.generation, "");
    }
}
