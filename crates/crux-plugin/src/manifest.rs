//! Plugin manifest: `~/.crux/plugins.toml` or per-project `.crux/plugins.toml`.
//!
//! Environment variables passed to plugins may be:
//!
//! - **Literals** — any plain string value (use only for non-secret config).
//! - **Env-refs** — values starting with `env://` are resolved from the host
//!   process environment at plugin load time. The suffix names the variable to
//!   look up, e.g. `env://GITHUB_TOKEN`. This keeps secrets out of manifest
//!   files; the manifest only records *which* variable to use, not the value.
//!
//! ```toml
//! [[plugin]]
//! name = "github"
//! path = "crux-github"
//! env = { GITHUB_TOKEN = "env://GITHUB_TOKEN", LOG_LEVEL = "info" }
//! ```
//!
//! Resolving env-refs happens in [`PluginEntry::resolve_env`], which is called
//! by the plugin host before spawning. A missing env-ref variable is a hard
//! error so misconfiguration is caught early and clearly.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

// ── EnvValue ─────────────────────────────────────────────────────────────────

const ENV_REF_PREFIX: &str = "env://";

/// A single environment variable value in a plugin manifest entry.
///
/// - `Literal` — a plain string, stored and passed as-is.
/// - `EnvRef`  — an indirection: resolved from the named host-process env var
///   at plugin load time, so secrets never appear in the manifest file.
///
/// `Debug` output redacts literal values to `[REDACTED]` to prevent secrets
/// from appearing in diagnostic output even when the variant is `Literal`.
#[derive(Clone, PartialEq, Eq)]
pub enum EnvValue {
    /// A non-secret config value to pass verbatim.
    Literal(String),
    /// Resolve the named env variable from the host process at load time.
    EnvRef(String),
}

impl std::fmt::Debug for EnvValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnvValue::Literal(_) => write!(f, "EnvValue::Literal([REDACTED])"),
            EnvValue::EnvRef(var) => write!(f, "EnvValue::EnvRef({var:?})"),
        }
    }
}

impl Serialize for EnvValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            EnvValue::Literal(v) => s.serialize_str(v),
            EnvValue::EnvRef(var) => s.serialize_str(&format!("{ENV_REF_PREFIX}{var}")),
        }
    }
}

impl<'de> Deserialize<'de> for EnvValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(if let Some(var) = raw.strip_prefix(ENV_REF_PREFIX) {
            EnvValue::EnvRef(var.to_owned())
        } else {
            EnvValue::Literal(raw)
        })
    }
}

// ── Manifest types ────────────────────────────────────────────────────────────

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
    ///
    /// Values may be literals or `env://VAR` references (see module docs).
    /// Call [`PluginEntry::resolve_env`] to expand refs before spawning.
    #[serde(default)]
    pub env: HashMap<String, EnvValue>,
    /// Sandbox policy applied when this plugin's subprocess is spawned.
    #[serde(default)]
    pub sandbox: SandboxPolicy,
}

impl PluginEntry {
    /// Resolve all [`EnvValue`] entries into concrete string values.
    ///
    /// `EnvRef` variants are looked up from the host process environment.
    /// Returns [`ManifestError::EnvRefMissing`] if any referenced variable is
    /// absent, so misconfiguration is caught before the plugin is spawned.
    pub fn resolve_env(&self) -> Result<HashMap<String, String>, ManifestError> {
        self.env
            .iter()
            .map(|(key, val)| {
                let resolved = match val {
                    EnvValue::Literal(v) => v.clone(),
                    EnvValue::EnvRef(var) => {
                        std::env::var(var).map_err(|_| ManifestError::EnvRefMissing {
                            plugin: self.name.clone(),
                            key: key.clone(),
                            var: var.clone(),
                        })?
                    }
                };
                Ok((key.clone(), resolved))
            })
            .collect()
    }
}

// ── Sandbox policy ───────────────────────────────────────────────────────────

/// Declarative sandbox policy for a plugin subprocess.
///
/// `env_allow` is enforced directly by the host at spawn time: the child
/// process environment is cleared (it does **not** inherit the host's full
/// environment) and only variables declared in [`PluginEntry::env`] plus
/// this allowlist are set. `fs_allow`, `network`, and `max_memory_mb` are
/// declared here and serialized into the `CRUX_SANDBOX_POLICY` environment
/// variable passed to the child for self-enforcement -- the host does not
/// currently provide OS-level filesystem jailing, network namespacing, or
/// resource-limit enforcement. That is tracked as follow-up work; treat
/// those three fields as advisory until a platform-specific backend lands.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SandboxPolicy {
    /// Extra environment variable names to pass through from the host
    /// process, beyond those declared in [`PluginEntry::env`].
    #[serde(default)]
    pub env_allow: Vec<String>,
    /// Filesystem paths the plugin is permitted to access. Advisory --
    /// forwarded to the plugin, not enforced by the OS.
    #[serde(default)]
    pub fs_allow: Vec<String>,
    /// Whether the plugin is permitted outbound network access. Advisory.
    #[serde(default)]
    pub network: NetworkPolicy,
    /// Advisory memory ceiling in megabytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u64>,
}

/// Network access policy declared for a plugin subprocess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicy {
    /// Plugin may make outbound network connections.
    #[default]
    Allow,
    /// Plugin must not make outbound network connections.
    Deny,
}

// ── Lifecycle ─────────────────────────────────────────────────────────────────

/// Lifecycle state of a loaded plugin subprocess.
///
/// Transitions: `Startup` -> `Running` on successful declare; `Running` ->
/// `Draining` when a reload begins; `Draining` -> `Running` once the reload
/// completes; any state -> `Failed` on an unrecoverable process error, and
/// `Failed` -> `Running` if a restart succeeds. `Shutdown` is terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginLifecycle {
    /// Subprocess has been spawned but has not yet completed handshake/declare.
    Startup,
    /// Subprocess is declared and accepting invocations.
    Running,
    /// A reload is in progress: in-flight calls are allowed to finish, but
    /// new calls are rejected until the new process is `Running`.
    Draining,
    /// The plugin has been shut down and will not be restarted.
    Shutdown,
    /// The subprocess died or became unresponsive and automatic restart
    /// either has not yet been attempted or has been exhausted.
    Failed,
}

impl PluginLifecycle {
    /// Whether invocations may be routed to the plugin in this state.
    pub fn accepts_invocations(self) -> bool {
        matches!(self, PluginLifecycle::Running)
    }
}

// ── Loader ────────────────────────────────────────────────────────────────────

/// Load a manifest from a TOML file path, returning `Default`
/// (empty) if the file doesn't exist.
pub fn load_manifest(path: impl AsRef<std::path::Path>) -> Result<PluginManifest, ManifestError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(PluginManifest::default());
    }
    let contents = std::fs::read_to_string(path)?;
    let manifest: PluginManifest = toml::from_str(&contents)?;
    Ok(manifest)
}

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("IO error reading manifest: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
    /// An `env://VAR` reference in the manifest could not be resolved because
    /// the named variable is absent from the host process environment.
    #[error("plugin '{plugin}': env key '{key}' references env var '{var}' which is not set")]
    EnvRefMissing {
        plugin: String,
        key: String,
        var: String,
    },
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_with_env(env: HashMap<String, EnvValue>) -> PluginEntry {
        PluginEntry {
            name: "test-plugin".into(),
            path: "test-bin".into(),
            env,
            sandbox: SandboxPolicy::default(),
        }
    }

    #[test]
    fn literal_value_round_trips_through_toml() {
        let toml = r#"
            [[plugin]]
            name = "p"
            path = "bin"
            [plugin.0.env]
            LOG_LEVEL = "debug"
        "#;
        let manifest: PluginManifest = toml::from_str(toml).expect("parse");
        let entry = &manifest.plugin[0];
        assert_eq!(entry.env["LOG_LEVEL"], EnvValue::Literal("debug".into()));
    }

    #[test]
    fn env_ref_value_round_trips_through_toml() {
        let toml = r#"
            [[plugin]]
            name = "p"
            path = "bin"
            [plugin.0.env]
            TOKEN = "env://MY_TOKEN"
        "#;
        let manifest: PluginManifest = toml::from_str(toml).expect("parse");
        let entry = &manifest.plugin[0];
        assert_eq!(entry.env["TOKEN"], EnvValue::EnvRef("MY_TOKEN".into()));
    }

    #[test]
    fn resolve_env_expands_literal() {
        let mut env = HashMap::new();
        env.insert("LOG_LEVEL".into(), EnvValue::Literal("info".into()));
        let entry = entry_with_env(env);
        let resolved = entry.resolve_env().expect("resolve");
        assert_eq!(resolved["LOG_LEVEL"], "info");
    }

    #[test]
    fn resolve_env_expands_ref_from_process_env() {
        // SAFETY: test-only, single-threaded within this test; the variable is
        // process-local and removed again immediately below.
        unsafe {
            std::env::set_var("CRUX_TEST_SECRET_VAR", "s3cr3t");
        }
        let mut env = HashMap::new();
        env.insert(
            "TOKEN".into(),
            EnvValue::EnvRef("CRUX_TEST_SECRET_VAR".into()),
        );
        let entry = entry_with_env(env);
        let resolved = entry.resolve_env().expect("resolve");
        assert_eq!(resolved["TOKEN"], "s3cr3t");
        // SAFETY: see above.
        unsafe {
            std::env::remove_var("CRUX_TEST_SECRET_VAR");
        }
    }

    #[test]
    fn resolve_env_errors_on_missing_ref() {
        // Ensure the variable is absent.
        // SAFETY: test-only, single-threaded within this test.
        unsafe {
            std::env::remove_var("CRUX_TEST_ABSENT_VAR");
        }
        let mut env = HashMap::new();
        env.insert(
            "TOKEN".into(),
            EnvValue::EnvRef("CRUX_TEST_ABSENT_VAR".into()),
        );
        let entry = entry_with_env(env);
        let err = entry.resolve_env().expect_err("should fail");
        assert!(
            matches!(err, ManifestError::EnvRefMissing { ref var, .. } if var == "CRUX_TEST_ABSENT_VAR"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn debug_output_redacts_literal_values() {
        let val = EnvValue::Literal("super-secret".into());
        let debug = format!("{val:?}");
        assert!(!debug.contains("super-secret"), "secret leaked: {debug}");
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn debug_output_shows_env_ref_variable_name() {
        let val = EnvValue::EnvRef("MY_TOKEN".into());
        let debug = format!("{val:?}");
        assert!(debug.contains("MY_TOKEN"), "var name missing: {debug}");
    }

    #[test]
    fn serialize_env_ref_produces_env_prefix() {
        let val = EnvValue::EnvRef("FOO".into());
        let s = toml::to_string(&val).expect("serialize");
        assert!(s.contains("env://FOO"), "got: {s}");
    }

    #[test]
    fn empty_manifest_loads_without_file() {
        let manifest = load_manifest("/nonexistent/path/plugins.toml").expect("load");
        assert!(manifest.plugin.is_empty());
    }
}
