//! BAML-backed structured extraction and decomposition handlers.
//!
//! The `llm::extract` handler is built dynamically from the definitions in
//! [`crate::definitions`] via [`crate::registry::BamlFunctionRegistry`].
//! Adding a new BAML function requires only a new factory in `definitions.rs`
//! — no match arm changes here.

use crate::baml_client::async_client::B;
use crate::definitions::all_definitions;
use crate::registry::BamlFunctionRegistry;
use baml::ClientRegistry;
use crux_runtime::prelude::CruxErr;
use crux_script::HandlerRegistry;
use serde_json::{Value, json};

/// Register the `llm::extract` handler with the default BAML client.
///
/// All generated BAML functions are registered automatically from
/// [`all_definitions`].
pub fn register_extract(registry: &mut HandlerRegistry) {
    register_extract_with(registry, None);
}

/// Register the `llm::extract` handler, optionally routing all calls through
/// the given [`ClientRegistry`].
///
/// All generated BAML functions are registered automatically from
/// [`all_definitions`].
pub fn register_extract_with(
    registry: &mut HandlerRegistry,
    client_registry: Option<ClientRegistry>,
) {
    let fn_registry = BamlFunctionRegistry::from_defs(all_definitions());
    fn_registry.register_extract_handler(registry, client_registry);
}

/// Register the `llm::decompose` handler.
pub fn register_decompose(registry: &mut HandlerRegistry) {
    registry.handler_value("llm::decompose", |input: Value| async move {
        let text = input
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CruxErr::step_failed("llm::decompose", "missing 'text' field"))?
            .to_string();

        let result = B
            .DecomposeSpec
            .call(text)
            .await
            .map_err(|e| CruxErr::step_failed("llm::decompose", format!("BAML error: {e}")))?;

        let tasks: Vec<Value> = result
            .tasks
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "name": t.name,
                    "title": t.title,
                    "description": t.description,
                    "priority": t.priority,
                    "status": t.status,
                    "files": t.files,
                })
            })
            .collect();

        Ok(json!({ "tasks": tasks }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_extract_does_not_panic() {
        let mut registry = HandlerRegistry::new();
        register_extract(&mut registry);
    }

    #[test]
    fn all_definitions_are_unique_and_non_empty() {
        let defs = all_definitions();
        assert!(!defs.is_empty(), "at least one definition must exist");
        let mut names: Vec<_> = defs.iter().map(|d| d.name).collect();
        let len_before = names.len();
        names.dedup();
        assert_eq!(names.len(), len_before, "definition names must be unique");
    }

    #[test]
    fn all_definitions_have_input_and_output_schemas() {
        for def in all_definitions() {
            assert!(
                def.input_schema.is_object(),
                "{}: input_schema must be a JSON object",
                def.name
            );
            assert!(
                def.output_schema.is_object(),
                "{}: output_schema must be a JSON object",
                def.name
            );
            assert!(
                def.timeout_ms > 0,
                "{}: timeout_ms must be positive",
                def.name
            );
        }
    }

    #[test]
    fn function_registry_lists_all_known_functions() {
        let defs = all_definitions();
        let expected_names = [
            "AssessHealth",
            "Classify",
            "ClassifyCIFailure",
            "ClassifyProject",
            "DescribeProject",
            "ExtractEntities",
            "GenerateChangelog",
            "SuggestRelated",
            "Summarize",
        ];
        let reg = BamlFunctionRegistry::from_defs(defs);
        let actual = reg.function_names();
        assert_eq!(actual, expected_names);
    }
}
