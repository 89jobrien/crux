use crate::{canonical::CanonicalModelId, error::ModelParseError, vendor::Vendor};

pub fn parse(raw: &str) -> Result<CanonicalModelId, ModelParseError> {
    // gpt-{gen}[-{variant}...]
    if let Some(rest) = raw.strip_prefix("gpt-") {
        // generation is up to the next '-' (if any)
        let (generation, variant) = match rest.find('-') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, ""),
        };
        return Ok(CanonicalModelId {
            vendor: Vendor::OpenAi,
            family: "gpt".to_string(),
            generation: generation.to_string(),
            variant: variant.to_string(),
        });
    }

    // o{N}[-{variant}...]  e.g. o1, o3-mini
    if let Some(rest) = raw.strip_prefix('o') {
        let digit_end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let digits = &rest[..digit_end];
        if !digits.is_empty() {
            let variant = if digit_end < rest.len() {
                // skip the '-'
                rest[digit_end..].trim_start_matches('-')
            } else {
                ""
            };
            return Ok(CanonicalModelId {
                vendor: Vendor::OpenAi,
                family: "o".to_string(),
                generation: digits.to_string(),
                variant: variant.to_string(),
            });
        }
    }

    Ok(super::fallback::parse(Vendor::OpenAi, raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vendor::Vendor;

    #[test]
    fn gpt_4o_mini() {
        let id = parse("gpt-4o-mini").unwrap();
        assert_eq!(id.vendor, Vendor::OpenAi);
        assert_eq!(id.family, "gpt");
        assert_eq!(id.generation, "4o");
        assert_eq!(id.variant, "mini");
    }

    #[test]
    fn gpt_4o() {
        let id = parse("gpt-4o").unwrap();
        assert_eq!(id.family, "gpt");
        assert_eq!(id.generation, "4o");
        assert_eq!(id.variant, "");
    }

    #[test]
    fn o3_mini() {
        let id = parse("o3-mini").unwrap();
        assert_eq!(id.family, "o");
        assert_eq!(id.generation, "3");
        assert_eq!(id.variant, "mini");
    }

    #[test]
    fn o1() {
        let id = parse("o1").unwrap();
        assert_eq!(id.family, "o");
        assert_eq!(id.generation, "1");
        assert_eq!(id.variant, "");
    }

    #[test]
    fn unknown_falls_back() {
        let id = parse("dall-e-3").unwrap();
        assert_eq!(id.family, "dall-e-3");
    }
}
