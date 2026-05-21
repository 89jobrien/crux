//! LLM-based pipeline planner — generates crux-script YAML from a goal string.

use crate::baml_client::async_client::B;
use crux_runtime::prelude::CruxErr;
use crux_script::HandlerRegistry;
use serde_json::{Value, json};

/// Register the `llm::plan` handler.
pub fn register_plan(registry: &mut HandlerRegistry, extra_handlers: Vec<String>) {
    registry.handler_value("llm::plan", move |input: Value| {
        let extra = extra_handlers.clone();
        async move {
            let goal = input
                .get("args")
                .and_then(|a| a.get("goal"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| CruxErr::step_failed("llm::plan", "missing 'goal' field"))?
                .to_string();

            let constraints = input
                .get("args")
                .and_then(|a| a.get("constraints"))
                .and_then(|v| v.as_str())
                .map(str::to_string);

            let mut handlers = handler_manifest();
            handlers.extend(extra);

            let result = B
                .GeneratePipeline
                .call(goal, &handlers, constraints)
                .await
                .map_err(|e| CruxErr::step_failed("llm::plan", format!("BAML: {e}")))?;

            let yaml = serde_yaml::to_string(&result)
                .map_err(|e| CruxErr::step_failed("llm::plan", format!("YAML serialize: {e}")))?;
            Ok(json!({
                "pipeline_name": result.pipeline,
                "yaml": yaml,
            }))
        }
    });
}

/// All registered handler names + descriptions for prompt injection.
fn handler_manifest() -> Vec<String> {
    vec![
        "shell::exec -- run shell command, return exit code".into(),
        "shell::capture -- run shell command, capture stdout".into(),
        "fs::read -- read file contents".into(),
        "fs::write -- write content to file".into(),
        "fs::glob -- glob pattern match, return file list".into(),
        "fs::exists -- check if path exists".into(),
        "git::staged_files -- list staged files".into(),
        "git::diff -- show diff".into(),
        "git::log -- show commit log".into(),
        "git::status -- show working tree status".into(),
        "json::pick -- extract field from JSON".into(),
        "json::merge -- merge JSON objects".into(),
        "json::jq -- jq-style query".into(),
        "ctrl::noop -- pass through unchanged".into(),
        "ctrl::log -- log input and pass through".into(),
        "ctrl::assert -- assert condition on input".into(),
        "llm::invoke -- raw LLM completion".into(),
        "llm::extract -- BAML structured extraction".into(),
        "llm::decompose -- decompose spec into tasks".into(),
        "llm::stream -- buffered LLM completion (streaming stub)".into(),
    ]
}

/// Generate pipeline YAML from a goal string.
pub async fn generate_pipeline(
    goal: &str,
    constraints: Option<&str>,
    extra_handlers: &[String],
) -> Result<String, CruxErr> {
    let mut handlers = handler_manifest();
    handlers.extend_from_slice(extra_handlers);
    let result = B
        .GeneratePipeline
        .call(goal.to_string(), &handlers, constraints.map(str::to_string))
        .await
        .map_err(|e| CruxErr::step_failed("llm::plan", format!("BAML: {e}")))?;
    serde_yaml::to_string(&result)
        .map_err(|e| CruxErr::step_failed("llm::plan", format!("YAML serialize: {e}")))
}
