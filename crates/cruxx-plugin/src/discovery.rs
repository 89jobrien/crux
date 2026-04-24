//! Plugin auto-discovery port and filesystem adapter.
//!
//! The `PluginDiscovery` trait is the port; `TomlFileDiscovery` is the
//! filesystem adapter. Tests use in-memory doubles by implementing the trait.

use crate::manifest::{ManifestError, PluginEntry, load_manifest};

/// Discovery port: returns the list of plugins to load.
///
/// Missing file → `Ok(empty)`. Parse failure → `Err`.
pub trait PluginDiscovery {
    fn discover(&self) -> Result<Vec<PluginEntry>, PluginDiscoveryError>;
}

/// Reads a TOML plugins file and returns its entries.
///
/// If the file does not exist, returns `Ok(vec![])`.
/// Returns `Err` only on a parse failure.
pub struct TomlFileDiscovery {
    pub path: std::path::PathBuf,
}

impl TomlFileDiscovery {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns a `TomlFileDiscovery` pointed at `~/.cruxx/plugins.toml`.
    /// Falls back to `./.cruxx/plugins.toml` if `HOME` is not set.
    pub fn default_path() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        Self::new(format!("{home}/.cruxx/plugins.toml"))
    }
}

impl PluginDiscovery for TomlFileDiscovery {
    fn discover(&self) -> Result<Vec<PluginEntry>, PluginDiscoveryError> {
        let manifest = load_manifest(&self.path)?;
        Ok(manifest.plugin)
    }
}

/// Errors returned by discovery.
#[derive(Debug, thiserror::Error)]
pub enum PluginDiscoveryError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn discovery_returns_empty_when_file_missing() {
        let disc = TomlFileDiscovery::new("/tmp/cruxx-nonexistent-plugins-xyz.toml");
        let entries = disc.discover().expect("should not error on missing file");
        assert!(entries.is_empty());
    }

    #[test]
    fn discovery_parses_valid_toml() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"
[[plugin]]
name = "github"
path = "cruxx-github"

[[plugin]]
name = "s3"
path = "/usr/local/bin/cruxx-s3"
env = {{ AWS_REGION = "us-east-1" }}
"#
        )
        .unwrap();

        let disc = TomlFileDiscovery::new(f.path());
        let entries = disc.discover().expect("parse should succeed");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "github");
        assert_eq!(entries[0].path, "cruxx-github");
        assert_eq!(entries[1].name, "s3");
        assert_eq!(entries[1].env.get("AWS_REGION").map(String::as_str), Some("us-east-1"));
    }

    #[test]
    fn discovery_errors_on_invalid_toml() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "this is not valid toml = [[[").unwrap();

        let disc = TomlFileDiscovery::new(f.path());
        assert!(disc.discover().is_err());
    }

    /// In-memory double — demonstrates the trait is testable without the filesystem.
    struct FixedDiscovery(Vec<PluginEntry>);

    impl PluginDiscovery for FixedDiscovery {
        fn discover(&self) -> Result<Vec<PluginEntry>, PluginDiscoveryError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn in_memory_double_implements_trait() {
        let entry = PluginEntry {
            name: "test".into(),
            path: "test-bin".into(),
            env: Default::default(),
        };
        let disc = FixedDiscovery(vec![entry]);
        let entries = disc.discover().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "test");
    }
}
