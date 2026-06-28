use crux_plugin::bridge::register_plugins;
use crux_plugin::discovery::{PluginDiscovery, TomlFileDiscovery};
use crux_runtime::prelude::*;
use crux_script::{HandlerRegistry, schema::PipelineDef, schema::StepDef};
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

    let mut unregistered: std::collections::HashSet<String> = std::collections::HashSet::new();
    for name in collect_handler_names(pipeline) {
        if reg.get_handler(&name).is_none() {
            if strict {
                unregistered.insert(name);
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

    if !unregistered.is_empty() {
        let mut sorted: Vec<String> = unregistered.into_iter().collect();
        sorted.sort();
        eprintln!(
            "[crux] error: --strict mode: unregistered handlers: {}",
            sorted.join(", ")
        );
        std::process::exit(1);
    }

    reg
}

/// Collect all handler/arm/stage names referenced in the pipeline.
pub fn collect_handler_names(pipeline: &PipelineDef) -> Vec<String> {
    let mut names = Vec::new();

    for step in &pipeline.steps {
        match step {
            StepDef::Step(node) => {
                names.push(node.handler.clone().unwrap_or_else(|| node.step.clone()));
            }
            StepDef::Delegate(node) => {
                names.push(node.delegate.clone());
            }
            StepDef::Pipe(node) => {
                names.extend(node.stages.iter().map(|a| a.handler_name().to_string()));
            }
            StepDef::JoinAll(node) => {
                names.extend(node.arms.iter().map(|a| a.handler_name().to_string()));
            }
            StepDef::RouteOnConfidence(node) => {
                for route in &node.routes {
                    names.push(route.handler.clone());
                }
            }
            StepDef::Speculate(node) => {
                names.extend(node.arms.iter().map(|a| a.handler_name().to_string()));
            }
        }
    }

    names.sort();
    names.dedup();
    names
}

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

/// Print a full execution trace with step details.
pub fn print_trace(crux: &Crux<Value>, elapsed: std::time::Duration) {
    println!("Pipeline: {}", crux.agent);
    println!(
        "Status:   {}",
        if crux.value().is_ok() { "OK" } else { "FAILED" }
    );
    println!("Duration: {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    println!("Steps:    {}", crux.steps.len());
    println!();

    println!("Trace:");
    for (i, step) in crux.steps.iter().enumerate() {
        let status = match step.status {
            StepStatus::Ok => "OK",
            StepStatus::Err => "ERR",
            StepStatus::Rejected => "REJ",
            StepStatus::Skipped => "SKIP",
        };
        let kind = match step.kind {
            StepKind::Plain => "",
            StepKind::Delegation => " [delegate]",
            StepKind::Branch => " [branch]",
            StepKind::Speculation => " [speculate]",
        };
        println!(
            "  {:>2}. [{:>4}] {}{} ({}ms)",
            i + 1,
            status,
            step.name,
            kind,
            step.duration_ms
        );
    }

    println!();
    match crux.value() {
        Ok(v) => {
            let pretty = serde_json::to_string_pretty(v).unwrap_or_default();
            println!("Output:\n{pretty}");
        }
        Err(e) => {
            println!("Error: {e}");
        }
    }
}
