//! Factory functions for every generated BAML function, each returning a
//! [`BamlFunctionDef`].
//!
//! ## Adding a new BAML function
//!
//! 1. Add a `fn <snake_name>_def() -> BamlFunctionDef` that captures the
//!    type-specific dispatch in its handler closure.
//! 2. Include it in [`all_definitions`].
//!
//! No other file needs to change.

use std::sync::Arc;

use baml::ClientRegistry;
use crux_runtime::prelude::CruxErr;
use crux_script::HandlerOutput;
use serde_json::{Map, Value, json};

use crate::baml_client::async_client::B;
use crate::registry::BamlFunctionDef;

// ---------------------------------------------------------------------------
// Helper macro: reduces the boilerplate for extracting a required string field
// from the input map and naming the BAML function in the error.
// ---------------------------------------------------------------------------
macro_rules! required_str {
    ($map:expr, $field:expr, $func:expr) => {
        $map.get($field)
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                CruxErr::step_failed(
                    "llm::extract",
                    format!("{} requires '{}' field", $func, $field),
                )
            })?
            .to_string()
    };
}

macro_rules! optional_str {
    ($map:expr, $field:expr) => {
        $map.get($field)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
}

macro_rules! string_vec {
    ($map:expr, $field:expr) => {
        $map.get($field)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
}

// ---------------------------------------------------------------------------
// Individual definition factories
// ---------------------------------------------------------------------------

fn extract_entities_def() -> BamlFunctionDef {
    BamlFunctionDef {
        name: "ExtractEntities",
        description: "Extract named entities from free text.",
        input_schema: json!({
            "type": "object",
            "required": ["text"],
            "properties": {
                "text": { "type": "string", "description": "The text to extract entities from." }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "entities": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "entity_type": { "type": "string" },
                            "description": { "type": "string" }
                        }
                    }
                }
            }
        }),
        timeout_ms: 15_000,
        handler: Arc::new(
            |input_map: Map<String, Value>, cr: Option<ClientRegistry>, co: Option<String>| {
                Box::pin(async move {
                    let text = required_str!(input_map, "text", "ExtractEntities");
                    let result = if let Some(reg) = cr {
                        B.ExtractEntities
                            .with_client_registry(&reg)
                            .call(text)
                            .await
                    } else if let Some(name) = co {
                        B.ExtractEntities.with_client(&name).call(text).await
                    } else {
                        B.ExtractEntities.call(text).await
                    }
                    .map_err(|e| {
                        CruxErr::step_failed("llm::extract", format!("BAML error: {e}"))
                    })?;

                    let entities: Vec<Value> = result
                        .entities
                        .iter()
                        .map(|e| {
                            json!({
                                "name": e.name,
                                "entity_type": e.entity_type,
                                "description": e.description,
                            })
                        })
                        .collect();
                    Ok(HandlerOutput::new(json!({ "entities": entities })))
                })
            },
        ),
    }
}

fn summarize_def() -> BamlFunctionDef {
    const DEFAULT_MAX_SENTENCES: i64 = 3;
    BamlFunctionDef {
        name: "Summarize",
        description: "Summarize a piece of text into key points.",
        input_schema: json!({
            "type": "object",
            "required": ["text"],
            "properties": {
                "text": { "type": "string" },
                "max_sentences": { "type": "integer", "default": 3 }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "key_points": { "type": "array", "items": { "type": "string" } },
                "word_count": { "type": "integer" }
            }
        }),
        timeout_ms: 15_000,
        handler: Arc::new(
            |input_map: Map<String, Value>, cr: Option<ClientRegistry>, co: Option<String>| {
                Box::pin(async move {
                    let text = required_str!(input_map, "text", "Summarize");
                    let max_sentences = input_map
                        .get("max_sentences")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(DEFAULT_MAX_SENTENCES);
                    let result = if let Some(reg) = cr {
                        B.Summarize
                            .with_client_registry(&reg)
                            .call(text, max_sentences)
                            .await
                    } else if let Some(name) = co {
                        B.Summarize
                            .with_client(&name)
                            .call(text, max_sentences)
                            .await
                    } else {
                        B.Summarize.call(text, max_sentences).await
                    }
                    .map_err(|e| {
                        CruxErr::step_failed("llm::extract", format!("BAML error: {e}"))
                    })?;
                    Ok(HandlerOutput::new(json!({
                        "summary": result.summary,
                        "key_points": result.key_points,
                        "word_count": result.word_count,
                    })))
                })
            },
        ),
    }
}

fn classify_def() -> BamlFunctionDef {
    BamlFunctionDef {
        name: "Classify",
        description: "Classify text into one of the provided labels with confidence.",
        input_schema: json!({
            "type": "object",
            "required": ["text", "labels"],
            "properties": {
                "text": { "type": "string" },
                "labels": { "type": "array", "items": { "type": "string" } }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "label": { "type": "string" },
                "confidence": { "type": "number" },
                "reasoning": { "type": "string" }
            }
        }),
        timeout_ms: 15_000,
        handler: Arc::new(
            |input_map: Map<String, Value>, cr: Option<ClientRegistry>, co: Option<String>| {
                Box::pin(async move {
                    let text = required_str!(input_map, "text", "Classify");
                    let labels: Vec<String> = input_map
                        .get("labels")
                        .and_then(|v| v.as_array())
                        .ok_or_else(|| {
                            CruxErr::step_failed("llm::extract", "Classify requires 'labels' array")
                        })?
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect();
                    let result = if let Some(reg) = cr {
                        B.Classify
                            .with_client_registry(&reg)
                            .call(text, &labels)
                            .await
                    } else if let Some(name) = co {
                        B.Classify.with_client(&name).call(text, &labels).await
                    } else {
                        B.Classify.call(text, &labels).await
                    }
                    .map_err(|e| {
                        CruxErr::step_failed("llm::extract", format!("BAML error: {e}"))
                    })?;
                    Ok(HandlerOutput::with_confidence(
                        json!({
                            "label": result.label,
                            "confidence": result.confidence,
                            "reasoning": result.reasoning,
                        }),
                        result.confidence as f32,
                    ))
                })
            },
        ),
    }
}

fn describe_project_def() -> BamlFunctionDef {
    BamlFunctionDef {
        name: "DescribeProject",
        description: "Generate a structured description for a software project.",
        input_schema: json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": { "type": "string" },
                "language": { "type": "string" },
                "readme": { "type": "string" },
                "commits": { "type": "array", "items": { "type": "string" } }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": { "description": { "type": "string" } }
        }),
        timeout_ms: 20_000,
        handler: Arc::new(
            |input_map: Map<String, Value>, cr: Option<ClientRegistry>, co: Option<String>| {
                Box::pin(async move {
                    let name = required_str!(input_map, "name", "DescribeProject");
                    let language = optional_str!(input_map, "language");
                    let readme = optional_str!(input_map, "readme");
                    let commits = string_vec!(input_map, "commits");
                    let result = if let Some(reg) = cr {
                        B.DescribeProject
                            .with_client_registry(&reg)
                            .call(name, language, readme, &commits)
                            .await
                    } else if let Some(n) = co {
                        B.DescribeProject
                            .with_client(&n)
                            .call(name, language, readme, &commits)
                            .await
                    } else {
                        B.DescribeProject
                            .call(name, language, readme, &commits)
                            .await
                    }
                    .map_err(|e| {
                        CruxErr::step_failed("llm::extract", format!("BAML error: {e}"))
                    })?;
                    Ok(HandlerOutput::new(
                        json!({ "description": result.description }),
                    ))
                })
            },
        ),
    }
}

fn assess_health_def() -> BamlFunctionDef {
    BamlFunctionDef {
        name: "AssessHealth",
        description: "Assess the health and activity status of a repository.",
        input_schema: json!({
            "type": "object",
            "required": ["name", "pushed_at"],
            "properties": {
                "name": { "type": "string" },
                "pushed_at": { "type": "string" },
                "commit_dates": { "type": "array", "items": { "type": "string" } },
                "open_issues": { "type": "integer" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "confidence": { "type": "number" },
                "reason": { "type": "string" }
            }
        }),
        timeout_ms: 15_000,
        handler: Arc::new(
            |input_map: Map<String, Value>, cr: Option<ClientRegistry>, co: Option<String>| {
                Box::pin(async move {
                    let name = required_str!(input_map, "name", "AssessHealth");
                    let pushed_at = required_str!(input_map, "pushed_at", "AssessHealth");
                    let commit_dates = string_vec!(input_map, "commit_dates");
                    let open_issues = input_map.get("open_issues").and_then(|v| v.as_i64());
                    let result = if let Some(reg) = cr {
                        B.AssessHealth
                            .with_client_registry(&reg)
                            .call(name, pushed_at, &commit_dates, open_issues)
                            .await
                    } else if let Some(n) = co {
                        B.AssessHealth
                            .with_client(&n)
                            .call(name, pushed_at, &commit_dates, open_issues)
                            .await
                    } else {
                        B.AssessHealth
                            .call(name, pushed_at, &commit_dates, open_issues)
                            .await
                    }
                    .map_err(|e| {
                        CruxErr::step_failed("llm::extract", format!("BAML error: {e}"))
                    })?;
                    Ok(HandlerOutput::with_confidence(
                        json!({
                            "status": result.status,
                            "confidence": result.confidence,
                            "reason": result.reason,
                        }),
                        result.confidence as f32,
                    ))
                })
            },
        ),
    }
}

fn classify_project_def() -> BamlFunctionDef {
    BamlFunctionDef {
        name: "ClassifyProject",
        description: "Classify a project into a category based on name, description, and topics.",
        input_schema: json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": { "type": "string" },
                "description": { "type": "string" },
                "language": { "type": "string" },
                "topics": { "type": "array", "items": { "type": "string" } },
                "commits": { "type": "array", "items": { "type": "string" } }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "category": { "type": "string" },
                "confidence": { "type": "number" }
            }
        }),
        timeout_ms: 15_000,
        handler: Arc::new(
            |input_map: Map<String, Value>, cr: Option<ClientRegistry>, co: Option<String>| {
                Box::pin(async move {
                    let name = required_str!(input_map, "name", "ClassifyProject");
                    let description = optional_str!(input_map, "description");
                    let language = optional_str!(input_map, "language");
                    let topics = string_vec!(input_map, "topics");
                    let commits = string_vec!(input_map, "commits");
                    let result = if let Some(reg) = cr {
                        B.ClassifyProject
                            .with_client_registry(&reg)
                            .call(name, description, language, &topics, &commits)
                            .await
                    } else if let Some(n) = co {
                        B.ClassifyProject
                            .with_client(&n)
                            .call(name, description, language, &topics, &commits)
                            .await
                    } else {
                        B.ClassifyProject
                            .call(name, description, language, &topics, &commits)
                            .await
                    }
                    .map_err(|e| {
                        CruxErr::step_failed("llm::extract", format!("BAML error: {e}"))
                    })?;
                    Ok(HandlerOutput::with_confidence(
                        json!({ "category": result.category, "confidence": result.confidence }),
                        result.confidence as f32,
                    ))
                })
            },
        ),
    }
}

fn generate_changelog_def() -> BamlFunctionDef {
    BamlFunctionDef {
        name: "GenerateChangelog",
        description: "Generate a structured changelog entry from commit messages.",
        input_schema: json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": { "type": "string" },
                "commits": { "type": "array", "items": { "type": "string" } }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "highlights": { "type": "array", "items": { "type": "string" } }
            }
        }),
        timeout_ms: 15_000,
        handler: Arc::new(
            |input_map: Map<String, Value>, cr: Option<ClientRegistry>, co: Option<String>| {
                Box::pin(async move {
                    let name = required_str!(input_map, "name", "GenerateChangelog");
                    let commits = string_vec!(input_map, "commits");
                    let result = if let Some(reg) = cr {
                        B.GenerateChangelog
                            .with_client_registry(&reg)
                            .call(name, &commits)
                            .await
                    } else if let Some(n) = co {
                        B.GenerateChangelog
                            .with_client(&n)
                            .call(name, &commits)
                            .await
                    } else {
                        B.GenerateChangelog.call(name, &commits).await
                    }
                    .map_err(|e| {
                        CruxErr::step_failed("llm::extract", format!("BAML error: {e}"))
                    })?;
                    Ok(HandlerOutput::new(json!({
                        "summary": result.summary,
                        "highlights": result.highlights,
                    })))
                })
            },
        ),
    }
}

fn suggest_related_def() -> BamlFunctionDef {
    BamlFunctionDef {
        name: "SuggestRelated",
        description: "Suggest related projects given a project's name, description, and category.",
        input_schema: json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": { "type": "string" },
                "description": { "type": "string" },
                "category": { "type": "string" },
                "all_projects": { "type": "array", "items": { "type": "string" } }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "related": { "type": "array", "items": { "type": "string" } }
            }
        }),
        timeout_ms: 15_000,
        handler: Arc::new(
            |input_map: Map<String, Value>, cr: Option<ClientRegistry>, co: Option<String>| {
                Box::pin(async move {
                    let name = required_str!(input_map, "name", "SuggestRelated");
                    let description = optional_str!(input_map, "description");
                    let category = optional_str!(input_map, "category");
                    let all_projects = string_vec!(input_map, "all_projects");
                    let result = if let Some(reg) = cr {
                        B.SuggestRelated
                            .with_client_registry(&reg)
                            .call(name, description, category, &all_projects)
                            .await
                    } else if let Some(n) = co {
                        B.SuggestRelated
                            .with_client(&n)
                            .call(name, description, category, &all_projects)
                            .await
                    } else {
                        B.SuggestRelated
                            .call(name, description, category, &all_projects)
                            .await
                    }
                    .map_err(|e| {
                        CruxErr::step_failed("llm::extract", format!("BAML error: {e}"))
                    })?;
                    Ok(HandlerOutput::new(json!({ "related": result.related })))
                })
            },
        ),
    }
}

fn classify_ci_failure_def() -> BamlFunctionDef {
    BamlFunctionDef {
        name: "ClassifyCIFailure",
        description: "Classify a CI failure output and suggest a fix.",
        input_schema: json!({
            "type": "object",
            "required": ["failure_output"],
            "properties": {
                "failure_output": { "type": "string" },
                "known_patterns": { "type": "array", "items": { "type": "string" } }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string" },
                "fix_type": { "type": "string" },
                "suggested_fix": { "type": "string" },
                "confidence": { "type": "number" },
                "new_pattern": { "type": "string" }
            }
        }),
        timeout_ms: 20_000,
        handler: Arc::new(
            |input_map: Map<String, Value>, cr: Option<ClientRegistry>, co: Option<String>| {
                Box::pin(async move {
                    let failure_output =
                        required_str!(input_map, "failure_output", "ClassifyCIFailure");
                    let known_patterns = string_vec!(input_map, "known_patterns");
                    let result = if let Some(reg) = cr {
                        B.ClassifyCIFailure
                            .with_client_registry(&reg)
                            .call(failure_output, &known_patterns)
                            .await
                    } else if let Some(n) = co {
                        B.ClassifyCIFailure
                            .with_client(&n)
                            .call(failure_output, &known_patterns)
                            .await
                    } else {
                        B.ClassifyCIFailure
                            .call(failure_output, &known_patterns)
                            .await
                    }
                    .map_err(|e| {
                        CruxErr::step_failed("llm::extract", format!("BAML error: {e}"))
                    })?;
                    Ok(HandlerOutput::with_confidence(
                        json!({
                            "kind": result.kind,
                            "fix_type": result.fix_type,
                            "suggested_fix": result.suggested_fix,
                            "confidence": result.confidence,
                            "new_pattern": result.new_pattern,
                        }),
                        result.confidence as f32,
                    ))
                })
            },
        ),
    }
}

// ---------------------------------------------------------------------------
// Public aggregator
// ---------------------------------------------------------------------------

/// Returns all generated BAML function definitions.
///
/// This is the single place that must be updated when a new BAML function is
/// added: add a factory function above and call it here.
pub fn all_definitions() -> Vec<BamlFunctionDef> {
    vec![
        extract_entities_def(),
        summarize_def(),
        classify_def(),
        describe_project_def(),
        assess_health_def(),
        classify_project_def(),
        generate_changelog_def(),
        suggest_related_def(),
        classify_ci_failure_def(),
    ]
}
