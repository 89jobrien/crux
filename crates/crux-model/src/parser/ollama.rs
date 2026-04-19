use crate::{
    canonical::CanonicalModelId, error::ModelParseError, provider_ref::ModelMetadata,
    vendor::Vendor,
};

pub fn parse(raw: &str) -> Result<CanonicalModelId, ModelParseError> {
    // Split on ':' to get name and tag
    let (name, tag) = match raw.split_once(':') {
        Some((n, t)) => (n, t),
        None => (raw, ""),
    };

    // Find where digits start in name to split family / generation
    let digit_start = name
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(name.len());
    let family_raw = &name[..digit_start];
    let after_family = &name[digit_start..];

    let family = family_raw.trim_end_matches('-').to_string();

    // generation = leading digits and dots
    let gen_end = after_family
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(after_family.len());
    let generation = after_family[..gen_end].to_string();
    let extra = after_family[gen_end..].trim_start_matches('-');

    let variant = match (extra.is_empty(), tag.is_empty()) {
        (true, true) => String::new(),
        (true, false) => tag.to_string(),
        (false, true) => extra.to_string(),
        (false, false) => format!("{extra}-{tag}"),
    };

    Ok(CanonicalModelId {
        vendor: Vendor::Ollama,
        family,
        generation,
        variant,
    })
}

pub fn enrich_from_api_entry(entry: &serde_json::Value) -> ModelMetadata {
    let details = &entry["details"];
    ModelMetadata {
        family: details["family"].as_str().map(str::to_string),
        parameter_size: details["parameter_size"].as_str().map(str::to_string),
        quantization: details["quantization_level"].as_str().map(str::to_string),
        format: details["format"].as_str().map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vendor::Vendor;

    #[test]
    fn llama3_2_latest() {
        let id = parse("llama3.2:latest").unwrap();
        assert_eq!(id.vendor, Vendor::Ollama);
        assert_eq!(id.family, "llama");
        assert_eq!(id.generation, "3.2");
        assert_eq!(id.variant, "latest");
    }

    #[test]
    fn qwen_coder_7b() {
        let id = parse("qwen2.5-coder:7b").unwrap();
        assert_eq!(id.family, "qwen");
        assert_eq!(id.generation, "2.5");
        assert_eq!(id.variant, "coder-7b");
    }

    #[test]
    fn no_tag() {
        let id = parse("mistral").unwrap();
        assert_eq!(id.family, "mistral");
        assert_eq!(id.variant, "");
    }

    #[test]
    fn enrich_from_api_response() {
        let json = serde_json::json!({
            "name": "llama3.2:latest",
            "details": {
                "family": "llama",
                "parameter_size": "8B",
                "quantization_level": "Q4_K_M",
                "format": "gguf"
            }
        });
        let meta = enrich_from_api_entry(&json);
        assert_eq!(meta.family.as_deref(), Some("llama"));
        assert_eq!(meta.parameter_size.as_deref(), Some("8B"));
        assert_eq!(meta.quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(meta.format.as_deref(), Some("gguf"));
    }
}
