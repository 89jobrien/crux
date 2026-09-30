use crate::{canonical::CanonicalModelId, error::ModelParseError, vendor::Vendor};

/// Parse Cohere command and embedding model IDs.
pub fn parse(raw: &str) -> Result<CanonicalModelId, ModelParseError> {
    if let Some(generation) = raw.strip_prefix("embed-v") {
        return Ok(canonical("embed", generation));
    }

    let segments: Vec<_> = raw.split('-').collect();
    if segments.len() >= 3
        && segments[segments.len() - 2]
            .chars()
            .all(|character| character.is_ascii_digit())
        && segments[segments.len() - 1]
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        let family = segments[..segments.len() - 2].join("-");
        let generation = segments[segments.len() - 2..].join("-");
        return Ok(canonical(&family, &generation));
    }

    Ok(super::fallback::parse(Vendor::Cohere, raw))
}

fn canonical(family: &str, generation: &str) -> CanonicalModelId {
    CanonicalModelId {
        vendor: Vendor::Cohere,
        family: family.to_string(),
        generation: generation.to_string(),
        variant: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_a_03_2025() {
        let id = parse("command-a-03-2025").unwrap();
        assert_eq!(id.family, "command-a");
        assert_eq!(id.generation, "03-2025");
    }

    #[test]
    fn embed_v4() {
        let id = parse("embed-v4.0").unwrap();
        assert_eq!(id.family, "embed");
        assert_eq!(id.generation, "4.0");
    }
}
