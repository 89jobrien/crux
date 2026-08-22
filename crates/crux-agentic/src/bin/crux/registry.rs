use crux_plugin::bridge::register_plugins;
use crux_plugin::discovery::{PluginDiscovery, TomlFileDiscovery};
use crux_runtime::prelude::*;
use crux_script::{HandlerOutput, HandlerRegistry, schema::PipelineDef, schema::StepDef};
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
                register_stub_handler(&mut reg, name);
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

/// Register a placeholder for a handler with no builtin.
///
/// The stub emits a mid-range confidence rather than none, so a pipeline whose
/// `route_on_confidence` keys off a feature-gated handler (`llm::extract`
/// without `--features baml`, say) still routes instead of failing. Registering
/// via `handler` rather than `handler_value` is what makes that confidence
/// visible to `{{ steps.<name>.confidence }}` -- `handler_value` sets it to
/// `None` and the expression errors.
pub fn register_stub_handler(reg: &mut HandlerRegistry, name: String) {
    const STUB_CONFIDENCE: f32 = 0.5;
    let n = name.clone();
    reg.handler(name, move |_input: Value| {
        let handler_name = n.clone();
        async move {
            eprintln!("[crux] warning: no builtin for '{handler_name}', using stub");
            Ok(HandlerOutput::with_confidence(
                json!({
                    "_stub": handler_name,
                    "confidence": STUB_CONFIDENCE,
                    "score": STUB_CONFIDENCE,
                }),
                STUB_CONFIDENCE,
            ))
        }
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The stub is what an unregistered handler resolves to on a default build
    /// (`llm::extract` without `--features baml`, for instance). It must carry a
    /// real confidence, not just mention one in its payload: registering via
    /// `handler_value` would set `HandlerOutput::confidence` to `None` and make
    /// `{{ steps.<name>.confidence }}` fail, taking down any pipeline that routes
    /// on a feature-gated handler.
    #[tokio::test]
    async fn stub_handler_emits_routable_confidence() {
        let mut reg = HandlerRegistry::new();
        register_stub_handler(&mut reg, "llm::extract".to_string());

        let handler = reg
            .get_handler("llm::extract")
            .expect("stub must be registered");
        let out = handler(Value::Null).await.expect("stub must not fail");

        assert_eq!(
            out.confidence,
            Some(0.5),
            "stub must expose confidence on HandlerOutput, not only inside its JSON"
        );
        assert_eq!(out.value["_stub"].as_str(), Some("llm::extract"));
    }
}
