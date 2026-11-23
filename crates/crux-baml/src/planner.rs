//! LLM-based pipeline planner — generates crux-script YAML from a goal string.

use crate::{
    baml_client::async_client::B, definitions::function_contracts, registry::BamlFunctionContract,
};
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

            let mut handlers = handler_manifest(&function_contracts());
            handlers.extend(extra);

            let result = B
                .GeneratePipeline
                .call(goal, &handlers, constraints)
                .await
                .map_err(|e| CruxErr::step_failed("llm::plan", format!("BAML: {e}")))?;

            let yaml = serde_saphyr::to_string(&result)
                .map_err(|e| CruxErr::step_failed("llm::plan", format!("YAML serialize: {e}")))?;
            Ok(json!({
                "pipeline_name": result.pipeline,
                "yaml": yaml,
            }))
        }
    });
}

/// Handler lines that do not depend on generated BAML functions.
fn static_handler_manifest() -> Vec<String> {
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
        "llm::analyze -- summarize evidence into severity-tagged findings, reports confidence".into(),
        "llm::confidence -- score how strongly evidence supports a claim, reports the score as confidence".into(),
        "llm::decompose -- decompose spec into tasks".into(),
        "llm::stream -- incremental streaming LLM completion".into(),
    ]
}

/// All registered handler names + descriptions for prompt injection.
///
/// One `llm::extract` line is emitted per generated BAML function, built from
/// its contract, so the planner sees every supported function and its required
/// inputs.
fn handler_manifest(contracts: &[BamlFunctionContract]) -> Vec<String> {
    let mut lines = static_handler_manifest();
    lines.extend(contracts.iter().map(|contract| {
        let required = contract.required_inputs();
        let required = if required.is_empty() {
            "none".to_string()
        } else {
            required.join(", ")
        };
        format!(
            "llm::extract (function \"{}\") -- {} Required input fields: {}.",
            contract.name, contract.description, required
        )
    }));
    lines
}

/// Generate pipeline YAML from a goal string.
pub async fn generate_pipeline(
    goal: &str,
    constraints: Option<&str>,
    extra_handlers: &[String],
) -> Result<String, CruxErr> {
    let mut handlers = handler_manifest(&function_contracts());
    handlers.extend_from_slice(extra_handlers);
    let result = B
        .GeneratePipeline
        .call(goal.to_string(), &handlers, constraints.map(str::to_string))
        .await
        .map_err(|e| CruxErr::step_failed("llm::plan", format!("BAML: {e}")))?;
    serde_saphyr::to_string(&result)
        .map_err(|e| CruxErr::step_failed("llm::plan", format!("YAML serialize: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_manifest_lists_every_generated_function_with_inputs() {
        let contracts = function_contracts();
        let manifest = handler_manifest(&contracts);
        for contract in &contracts {
            let quoted = format!("\"{}\"", contract.name);
            let line = manifest
                .iter()
                .find(|l| l.contains(&quoted))
                .unwrap_or_else(|| panic!("no manifest line for {}", contract.name));
            for field in contract.required_inputs() {
                assert!(
                    line.contains(field),
                    "{} manifest line should mention required input {field}",
                    contract.name
                );
            }
        }
    }

    #[test]
    fn handler_manifest_keeps_static_handlers() {
        let manifest = handler_manifest(&[]);
        assert!(manifest.iter().any(|l| l.starts_with("fs::read")));
        assert!(manifest.iter().any(|l| l.starts_with("llm::invoke")));
    }
}
