//! Handlers for commit categorization, true/false positive classification,
//! and allowlist entry generation.

use crux_script::{HandlerMetadata, HandlerOutput, HandlerRegistry, RiskLevel};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::handlers;

pub(super) fn register(registry: &mut HandlerRegistry) {
    register_categorize_commits(registry);
    register_classify_true_false(registry);
    register_generate_allowlist_entries(registry);
}

fn register_categorize_commits(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::TRIAGE_CATEGORIZE_COMMITS)
            .describe("Categorize commit log lines by conventional commit prefix.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let items = input
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut categories: HashMap<String, Vec<Value>> = HashMap::new();
            for item in &items {
                let output = item.get("output").and_then(|v| v.as_str()).unwrap_or("");
                for line in output.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let category = if line.starts_with("feat") {
                        "feat"
                    } else if line.starts_with("fix") {
                        "fix"
                    } else if line.starts_with("chore") {
                        "chore"
                    } else if line.starts_with("docs") {
                        "docs"
                    } else if line.starts_with("refactor") {
                        "refactor"
                    } else if line.starts_with("test") {
                        "test"
                    } else {
                        "other"
                    };
                    categories
                        .entry(category.to_string())
                        .or_default()
                        .push(json!({"line": line}));
                }
            }

            let map: serde_json::Map<String, Value> = categories
                .into_iter()
                .map(|(k, v)| (k, Value::Array(v)))
                .collect();
            Ok(Value::Object(map))
        },
    );
}

fn register_classify_true_false(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_CLASSIFY_TRUE_FALSE)
            .describe(
                "Classify obfsck findings as true or false positives using file and context heuristics.",
            )
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler(
        handlers::TRIAGE_CLASSIFY_TRUE_FALSE,
        |input: Value| async move {
            let items = input
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            // Heuristics for false positives
            let false_positive_indicators = [
                "test",
                "fixture",
                "localhost",
                "127.0.0.1",
                "example",
                "password",
                "dummy",
                "fake",
                "mock",
                "sample",
            ];

            let mut true_positives = Vec::new();
            let mut false_positives = Vec::new();

            for item in &items {
                let file = item.get("file").and_then(|f| f.as_str()).unwrap_or("");
                let pattern = item.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
                let context = item.get("context").and_then(|c| c.as_str()).unwrap_or("");

                let combined = format!("{file} {pattern} {context}").to_lowercase();
                let is_fp = false_positive_indicators
                    .iter()
                    .any(|ind| combined.contains(ind))
                    || file.contains("tests/")
                    || file.contains("_test.")
                    || file.ends_with(".md");

                if is_fp {
                    false_positives.push(item.clone());
                } else {
                    true_positives.push(item.clone());
                }
            }

            let total = items.len() as f64;
            let fp_ratio = if total == 0.0 {
                1.0
            } else {
                false_positives.len() as f64 / total
            };
            let confidence = fp_ratio as f32; // 1.0 = all false positives

            Ok(HandlerOutput::with_confidence(
                json!({"true_positives": true_positives, "false_positives": false_positives}),
                confidence,
            ))
        },
    );
}

fn register_generate_allowlist_entries(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::TRIAGE_GENERATE_ALLOWLIST_ENTRIES)
            .describe("Generate obfsck allowlist pathspec entries from false-positive findings.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let fps = input
                .get("false_positives")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let entries: Vec<Value> = fps
                .iter()
                .map(|fp| {
                    let file = fp.get("file").and_then(|f| f.as_str()).unwrap_or("*");
                    let pattern = fp.get("pattern").and_then(|p| p.as_str()).unwrap_or("*");
                    json!({
                        "file": file,
                        "pattern": pattern,
                        "allowlist_entry": format!(":!{file}"),
                        "reason": "false positive — test fixture or documentation",
                    })
                })
                .collect();

            Ok(json!({"allowlist_entries": entries}))
        },
    );
}
