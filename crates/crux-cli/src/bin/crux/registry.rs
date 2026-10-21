//! Built-in and plugin handler registry construction for CLI commands.

use crux_plugin::bridge::register_plugins;
use crux_plugin::discovery::{PluginDiscovery, PluginDiscoveryError, TomlFileDiscovery};
use crux_plugin::host::PluginError;
use crux_script::{HandlerRegistry, schema::PipelineDef};

#[derive(Debug)]
pub enum RegistryBuildError {
    Discovery(PluginDiscoveryError),
    Plugin(PluginError),
}

impl std::fmt::Display for RegistryBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discovery(error) => write!(formatter, "failed to discover plugins: {error}"),
            Self::Plugin(error) => write!(formatter, "failed to register plugins: {error}"),
        }
    }
}

impl std::error::Error for RegistryBuildError {}

impl From<PluginDiscoveryError> for RegistryBuildError {
    fn from(error: PluginDiscoveryError) -> Self {
        Self::Discovery(error)
    }
}

impl From<PluginError> for RegistryBuildError {
    fn from(error: PluginError) -> Self {
        Self::Plugin(error)
    }
}

/// Resolve the plugins.toml path from an explicit flag or the default location.
pub fn resolve_plugins_path(plugins_path: Option<&str>) -> String {
    plugins_path.map(String::from).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.crux/plugins.toml")
    })
}

/// Build a registry seeded with all crux-agentic built-in handlers.
pub async fn build_base_registry(plugins_path: Option<&str>) -> HandlerRegistry {
    match build_registry(plugins_path).await {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("[crux] warning: {error}");
            let mut registry = HandlerRegistry::new();
            crux_agentic::register_all_with_plugins(&mut registry, Vec::new());
            registry
        }
    }
}

/// Build the execution registry, propagating discovery and plugin errors.
pub async fn build_registry(
    plugins_path: Option<&str>,
) -> Result<HandlerRegistry, RegistryBuildError> {
    let disc = TomlFileDiscovery::new(resolve_plugins_path(plugins_path));
    let entries = disc.discover()?;
    let manifest = crux_plugin::manifest::PluginManifest { plugin: entries };

    let plugin_handler_descs: Vec<String> = manifest
        .plugin
        .iter()
        .map(|p| format!("{}::* -- plugin (see plugins.toml)", p.name))
        .collect();

    let mut reg = HandlerRegistry::new();
    crux_agentic::register_all_with_plugins(&mut reg, plugin_handler_descs);

    if !manifest.plugin.is_empty() {
        register_plugins(&mut reg, &manifest.plugin).await?;
    }

    Ok(reg)
}

pub use crux_script::collect_handler_names;

/// Warn if the pipeline uses LLM handlers but no API keys are set.
pub fn warn_missing_env(pipeline: &PipelineDef) {
    let handlers = collect_handler_names(pipeline);
    let needs_llm = handlers.iter().any(|h| h.starts_with("llm::"));
    if !needs_llm {
        return;
    }

    let has_openai = std::env::var("OPENAI_API_KEY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let has_anthropic = std::env::var("ANTHROPIC_API_KEY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);

    if !has_openai && !has_anthropic {
        eprintln!(
            "[crux] warning: pipeline uses llm:: handlers but neither \
             OPENAI_API_KEY nor ANTHROPIC_API_KEY is set"
        );
        eprintln!(
            "[crux] hint: copy .env.example to .env and configure, \
             or use `dotenvx run -- crux run ...`"
        );
    }
}
