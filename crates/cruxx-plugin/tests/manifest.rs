use cruxx_plugin::manifest::PluginManifest;

#[test]
fn parse_minimal_manifest() {
    let toml = r#"
[[plugin]]
name = "github"
path = "/usr/local/bin/cruxx-github"
"#;
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    assert_eq!(manifest.plugin.len(), 1);
    assert_eq!(manifest.plugin[0].name, "github");
    assert_eq!(manifest.plugin[0].path, "/usr/local/bin/cruxx-github");
    assert!(manifest.plugin[0].env.is_empty());
}

#[test]
fn parse_manifest_with_env() {
    let toml = r#"
[[plugin]]
name = "slack"
path = "cruxx-slack"
env = { SLACK_TOKEN = "xoxb-test" }
"#;
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    assert_eq!(manifest.plugin[0].env["SLACK_TOKEN"], "xoxb-test");
}

#[test]
fn parse_manifest_multiple_plugins() {
    let toml = r#"
[[plugin]]
name = "github"
path = "cruxx-github"

[[plugin]]
name = "linear"
path = "cruxx-linear"
"#;
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    assert_eq!(manifest.plugin.len(), 2);
    assert_eq!(manifest.plugin[0].name, "github");
    assert_eq!(manifest.plugin[1].name, "linear");
}

#[test]
fn parse_empty_manifest() {
    let toml = "";
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    assert!(manifest.plugin.is_empty());
}
