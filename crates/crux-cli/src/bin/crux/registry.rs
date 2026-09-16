use crux_plugin::bridge::register_plugins;
use crux_plugin::discovery::{PluginDiscovery, TomlFileDiscovery};
use crux_script::{HandlerRegistry, collect_agent_names, schema::PipelineDef};
use serde_json::{Value, json};

/// Resolve the plugins.toml path from an explicit flag or the default location.
pub fn resolve_plugins_path(plugins_path: Option<&str>) -> String {
    plugins_path.map(String::from).unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.crux/plugins.toml")
    })
}

/// Build a registry seeded with all crux-agentic built-in handlers.
pub async fn build_registry(
    pipeline: &PipelineDef,
    plugins_path: Option<&str>,
    strict: bool,
) -> HandlerRegistry {
    let disc = TomlFileDiscovery::new(resolve_plugins_path(plugins_path));
    let entries = disc.discover().unwrap_or_default();
    let manifest = crux_plugin::manifest::PluginManifest { plugin: entries };

    let plugin_handler_descs: Vec<String> = manifest
        .plugin
        .iter()
        .map(|p| format!("{}::* -- plugin (see plugins.toml)", p.name))
        .collect();

    let mut reg = HandlerRegistry::new();
    crux_agentic::register_all_with_plugins(&mut reg, plugin_handler_descs);

    if !manifest.plugin.is_empty()
        && let Err(e) = register_plugins(&mut reg, &manifest.plugin).await
    {
        eprintln!("[crux] warning: failed to load plugins: {e}");
    }

    let mut unregistered_handlers = std::collections::HashSet::new();
    for name in collect_handler_names(pipeline) {
        if reg.get_handler(&name).is_none() {
            if strict {
                unregistered_handlers.insert(name);
            } else {
                let n = name.clone();
                reg.handler_value(name, move |_input: Value| {
                    let handler_name = n.clone();
                    async move {
                        eprintln!("[crux] warning: no builtin for '{handler_name}', using stub");
                        Ok(json!({
                            "_stub": handler_name,
                            "confidence": 0.5,
                            "score": 0.5,
                        }))
                    }
                });
            }
        }
    }

    let mut unregistered_agents = std::collections::HashSet::new();
    for name in collect_agent_names(pipeline) {
        if reg.get_agent(&name).is_none() {
            if strict {
                unregistered_agents.insert(name);
            } else {
                let n = name.clone();
                reg.agent_fn(name, move |_input: Value| {
                    let agent_name = n.clone();
                    async move {
                        eprintln!(
                            "[crux] warning: no builtin for agent '{agent_name}', using stub"
                        );
                        Ok(json!({
                            "_stub": agent_name,
                            "confidence": 0.5,
                            "score": 0.5,
                        }))
                    }
                });
            }
        }
    }

    if !unregistered_handlers.is_empty() || !unregistered_agents.is_empty() {
        let mut sorted: Vec<String> = unregistered_handlers.into_iter().collect();
        sorted.sort();
        if !sorted.is_empty() {
            eprintln!(
                "[crux] error: --strict mode: unregistered handlers: {}",
                sorted.join(", ")
            );
        }
        let mut agents: Vec<String> = unregistered_agents.into_iter().collect();
        agents.sort();
        if !agents.is_empty() {
            eprintln!(
                "[crux] error: --strict mode: unregistered agents: {}",
                agents.join(", ")
            );
        }
        std::process::exit(1);
    }

    reg
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
