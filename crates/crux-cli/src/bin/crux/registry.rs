use crux_plugin::bridge::register_plugins;
use crux_plugin::discovery::{PluginDiscovery, TomlFileDiscovery};
use crux_runtime::prelude::*;
use crux_script::{
    HandlerRegistry,
    schema::{DisplayOutput, PipelineDef, PipelineDisplayDef, StepDef},
};
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
    collect_handler_names_into(&pipeline.steps, &mut names);
    names.sort();
    names.dedup();
    names
}

fn collect_handler_names_into(steps: &[StepDef], names: &mut Vec<String>) {
    for step in steps {
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
            StepDef::Poll(node) => {
                collect_handler_names_into(&node.steps, names);
            }
            StepDef::ForEach(node) => {
                collect_handler_names_into(&node.steps, names);
            }
            StepDef::While(node) => {
                collect_handler_names_into(&node.steps, names);
            }
            StepDef::Repeat(node) => {
                collect_handler_names_into(&node.steps, names);
            }
        }
    }
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

fn display_title<'a>(crux: &'a Crux<Value>, display: Option<&'a PipelineDisplayDef>) -> &'a str {
    display
        .and_then(|metadata| metadata.title.as_deref())
        .unwrap_or(&crux.agent)
}

fn display_step_name<'a>(name: &'a str, display: Option<&'a PipelineDisplayDef>) -> &'a str {
    display
        .and_then(|metadata| metadata.steps.get(name))
        .map(String::as_str)
        .unwrap_or(name)
}

fn format_duration(duration: std::time::Duration) -> String {
    if duration.as_secs() >= 1 {
        format!("{:.2}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn is_shell_result(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.contains_key("exit_code")
            && object.contains_key("stdout")
            && object.contains_key("stderr")
    })
}

fn should_render_output(value: &Value, display: Option<&PipelineDisplayDef>) -> bool {
    match display.map_or(DisplayOutput::Auto, |metadata| metadata.output) {
        DisplayOutput::Auto => !is_shell_result(value),
        DisplayOutput::Always => true,
        DisplayOutput::Never => false,
    }
}

/// Render concise human-facing pipeline output for the default CLI mode.
pub fn render_summary(
    crux: &Crux<Value>,
    elapsed: std::time::Duration,
    display: Option<&PipelineDisplayDef>,
) -> String {
    let mut out = String::new();
    let status = if crux.value().is_ok() { "PASS" } else { "FAIL" };
    let title = display_title(crux, display);
    out.push_str(&format!(
        "{title}  {status}  {}\n\n",
        format_duration(elapsed)
    ));

    for step in &crux.steps {
        let icon = match step.status {
            StepStatus::Ok => "✓",
            StepStatus::Err => "✗",
            StepStatus::Rejected => "·",
            StepStatus::Skipped => "-",
        };
        let name = display_step_name(&step.name, display);
        let duration = format_duration(std::time::Duration::from_millis(step.duration_ms));
        out.push_str(&format!("  {icon} {name:<42} {duration:>8}\n"));
    }

    let passed = crux
        .steps
        .iter()
        .filter(|step| step.status == StepStatus::Ok)
        .count();
    out.push_str(&format!("\n{passed}/{} checks passed\n", crux.steps.len()));

    match crux.value() {
        Ok(value) if should_render_output(value, display) => {
            let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
            out.push_str(&format!("\nOutput:\n{pretty}\n"));
        }
        Ok(_) => {}
        Err(_) => {}
    }

    out
}

/// Render the full trace envelope (pipeline info, per-step status, timing, output) as text.
///
/// Pure: no I/O. Used for `--verbose`/`-v` output.
pub fn render_trace(
    crux: &Crux<Value>,
    elapsed: std::time::Duration,
    display: Option<&PipelineDisplayDef>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("Pipeline: {}\n", display_title(crux, display)));
    out.push_str(&format!(
        "Status:   {}\n",
        if crux.value().is_ok() { "OK" } else { "FAILED" }
    ));
    out.push_str(&format!(
        "Duration: {:.1}ms\n",
        elapsed.as_secs_f64() * 1000.0
    ));
    out.push_str(&format!("Steps:    {}\n\n", crux.steps.len()));

    out.push_str("Trace:\n");
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
        let name = display_step_name(&step.name, display);
        out.push_str(&format!(
            "  {:>2}. [{:>4}] {}{} ({}ms)\n",
            i + 1,
            status,
            name,
            kind,
            step.duration_ms
        ));
    }

    out.push('\n');
    if let Ok(v) = crux.value() {
        let pretty = serde_json::to_string_pretty(v).unwrap_or_default();
        out.push_str(&format!("Output:\n{pretty}\n"));
    }

    out
}
