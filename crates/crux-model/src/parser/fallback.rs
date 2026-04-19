use crate::{canonical::CanonicalModelId, vendor::Vendor};

pub fn parse(vendor: Vendor, raw: &str) -> CanonicalModelId {
    CanonicalModelId {
        vendor,
        family: raw.to_string(),
        generation: String::new(),
        variant: String::new(),
    }
}
