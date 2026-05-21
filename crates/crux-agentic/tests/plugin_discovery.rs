//! Integration test: TomlFileDiscovery used from crux-agentic context.

use std::io::Write as _;

use crux_plugin::discovery::{PluginDiscovery, TomlFileDiscovery};
use tempfile::NamedTempFile;

#[test]
fn toml_file_discovery_returns_entries_from_valid_manifest() {
    let mut f = NamedTempFile::new().unwrap();
    writeln!(
        f,
        r#"
[[plugin]]
name = "my-plugin"
path = "/usr/local/bin/my-plugin"
"#
    )
    .unwrap();

    let disc = TomlFileDiscovery::new(f.path());
    let entries = disc.discover().expect("discovery should succeed");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "my-plugin");
    assert_eq!(entries[0].path, "/usr/local/bin/my-plugin");
}

#[test]
fn toml_file_discovery_returns_empty_for_nonexistent_path() {
    let disc = TomlFileDiscovery::new("/tmp/crux-no-such-plugins-file-xyz.toml");
    let entries = disc
        .discover()
        .expect("missing file should return Ok(empty)");
    assert!(entries.is_empty());
}
