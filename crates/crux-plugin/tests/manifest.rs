//! Parsing tests for minimal, environment-aware, and multi-plugin manifests.

use crux_plugin::manifest::{EnvValue, NetworkPolicy, PluginManifest};

#[test]
fn parse_minimal_manifest() {
    let toml = r#"
[[plugin]]
name = "github"
path = "/usr/local/bin/crux-github"
"#;
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    assert_eq!(manifest.plugin.len(), 1);
    assert_eq!(manifest.plugin[0].name, "github");
    assert_eq!(manifest.plugin[0].path, "/usr/local/bin/crux-github");
    assert!(manifest.plugin[0].env.is_empty());
}

#[test]
fn parse_manifest_with_env() {
    let toml = r#"
[[plugin]]
name = "slack"
path = "crux-slack"
env = { SLACK_TOKEN = "xoxb-test" }
"#;
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    assert_eq!(
        manifest.plugin[0].env["SLACK_TOKEN"],
        EnvValue::Literal("xoxb-test".into())
    );
}

#[test]
fn parse_manifest_multiple_plugins() {
    let toml = r#"
[[plugin]]
name = "github"
path = "crux-github"

[[plugin]]
name = "linear"
path = "crux-linear"
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

#[test]
fn plugin_without_sandbox_table_gets_default_policy() {
    let toml = r#"
[[plugin]]
name = "github"
path = "crux-github"
"#;
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    let sandbox = &manifest.plugin[0].sandbox;
    assert!(sandbox.env_allow.is_empty());
    assert!(sandbox.fs_allow.is_empty());
    assert_eq!(sandbox.network, NetworkPolicy::Allow);
    assert_eq!(sandbox.max_memory_mb, None);
}

#[test]
fn parse_sandbox_table_with_all_fields() {
    let toml = r#"
[[plugin]]
name = "s3"
path = "crux-s3"
sandbox.env_allow = ["AWS_PROFILE"]
sandbox.fs_allow = ["/tmp/crux-s3"]
sandbox.network = "deny"
sandbox.max_memory_mb = 256
"#;
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    let sandbox = &manifest.plugin[0].sandbox;
    assert_eq!(sandbox.env_allow, vec!["AWS_PROFILE".to_string()]);
    assert_eq!(sandbox.fs_allow, vec!["/tmp/crux-s3".to_string()]);
    assert_eq!(sandbox.network, NetworkPolicy::Deny);
    assert_eq!(sandbox.max_memory_mb, Some(256));
}

#[test]
fn sandbox_network_policy_defaults_to_allow_when_omitted() {
    let toml = r#"
[[plugin]]
name = "github"
path = "crux-github"

[plugin.0.sandbox]
fs_allow = ["/tmp"]
"#;
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    assert_eq!(manifest.plugin[0].sandbox.network, NetworkPolicy::Allow);
}
