//! Plugin manifest: `~/.crux/plugins.toml` or per-project `.crux/plugins.toml`.
//!
//! ```toml
//! [[plugin]]
//! name = "github"
//! path = "crux-github"          # binary name or absolute path
//! env = { GITHUB_TOKEN = "..." } # optional env vars for the process
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Top-level manifest file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginManifest {
    #[serde(default)]
    pub plugin: Vec<PluginEntry>,
}

/// A single plugin entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    /// Plugin name, used as the handler namespace prefix.
    pub name: String,
    /// Path to the plugin binary (absolute or on PATH).
    pub path: String,
    /// Environment variables to pass to the plugin process.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Load a manifest from a TOML file path, returning `Default`
/// (empty) if the file doesn't exist.
pub fn load_manifest(
    path: impl AsRef<std::path::Path>,
) -> Result<PluginManifest, ManifestError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(PluginManifest::default());
    }
    let contents = std::fs::read_to_string(path)?;
    let manifest: PluginManifest = toml::from_str(&contents)?;
    Ok(manifest)
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("IO error reading manifest: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
}
