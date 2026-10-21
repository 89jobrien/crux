//! Lossless fallback normalization for unrecognized provider model names.

use crate::{canonical::CanonicalModelId, vendor::Vendor};

/// Preserves the raw provider ID as the canonical family when no parser matches.
pub fn parse(vendor: Vendor, raw: &str) -> CanonicalModelId {
    CanonicalModelId {
        vendor,
        family: raw.to_string(),
        generation: String::new(),
        variant: String::new(),
    }
}
