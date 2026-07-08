use crate::{canonical::CanonicalModelId, error::ModelParseError, vendor::Vendor};

/// Mistral naming: `{family}-{tier}[-{version}]`.
///
/// "large", "small", "latest" are tiers/tags, not numeric generations.
/// The split at the first `-` places the tier (e.g. `large`) and any
/// trailing version (e.g. `2407`) together in `variant`, and `generation`
/// is left empty. This is intentional: Mistral does not use numeric
/// generation numbers in the same sense as Anthropic or OpenAI, so
/// conflating tier with version in `variant` preserves the raw identity
/// without information loss.
///
/// Example: `mistral-large-2407` → `family="mistral"`, `variant="large-2407"`.
pub fn parse(raw: &str) -> Result<CanonicalModelId, ModelParseError> {
    let (family, variant) = match raw.split_once('-') {
        Some((f, rest)) => (f.to_string(), rest.to_string()),
        None => (raw.to_string(), String::new()),
    };

    Ok(CanonicalModelId {
        vendor: Vendor::Mistral,
        family,
        generation: String::new(),
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
        assert_eq!(id.generation, "");
        assert_eq!(id.variant, "large-latest");
    }

    #[test]
    fn codestral_latest() {
        let id = parse("codestral-latest").unwrap();
        assert_eq!(id.family, "codestral");
        assert_eq!(id.generation, "");
        assert_eq!(id.variant, "latest");
    }

    #[test]
    fn single_word() {
        let id = parse("mistral").unwrap();
        assert_eq!(id.family, "mistral");
        assert_eq!(id.generation, "");
        assert_eq!(id.variant, "");
    }
}
