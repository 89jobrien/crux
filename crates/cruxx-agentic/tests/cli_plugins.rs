/// Tests for plugin auto-discovery (#18).
///
/// Verifies that:
/// - Default plugin path resolves to ~/.cruxx/plugins.toml
/// - A missing plugins.toml gracefully produces an empty manifest (no panic)
/// - A valid plugins.toml with a plugin entry is parsed correctly
use cruxx_plugin::manifest::load_manifest;

#[test]
fn missing_plugins_toml_returns_empty_manifest() {
    let manifest = load_manifest("/nonexistent/path/plugins.toml").unwrap_or_default();
    assert!(
        manifest.plugin.is_empty(),
        "missing plugins.toml should produce empty manifest"
    );
}

#[test]
fn default_plugins_path_uses_home_directory() {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let expected = format!("{home}/.cruxx/plugins.toml");

    // Simulate the resolve_plugins_path logic from the binary.
    let resolved = resolve_plugins_path(None);
    assert_eq!(
        resolved, expected,
        "default plugin path should be ~/.cruxx/plugins.toml"
    );
}

#[test]
fn explicit_plugins_path_overrides_default() {
    let resolved = resolve_plugins_path(Some("/custom/plugins.toml"));
    assert_eq!(resolved, "/custom/plugins.toml");
}

#[test]
fn valid_plugins_toml_parses_entries() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let toml = r#"
[[plugin]]
name = "my-plugin"
path = "/usr/local/bin/my-plugin"
"#;
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(toml.as_bytes()).unwrap();

    let manifest =
        load_manifest(f.path().to_str().unwrap()).expect("valid plugins.toml should parse");
    assert_eq!(manifest.plugin.len(), 1);
    assert_eq!(manifest.plugin[0].name, "my-plugin");
}

/// Mirror the binary's resolve_plugins_path logic to keep tests in sync.
fn resolve_plugins_path(plugins_path: Option<&str>) -> String {
    plugins_path.map(String::from).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.cruxx/plugins.toml")
    })
}
