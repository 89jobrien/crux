use crux_script::{HandlerMetadata, HandlerOutput, HandlerRegistry, RiskLevel, SideEffect};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use crate::handlers;

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::CI_COMPILE_ERRORS)
            .describe("Parse rustc error output into structured compile error records.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let log = input.get("log").and_then(|l| l.as_str()).unwrap_or("");
            let mut errors = Vec::new();
            let lines: Vec<&str> = log.lines().collect();

            for (i, line) in lines.iter().enumerate() {
                if let Some(rest) = line.trim().strip_prefix("error[")
                    && let Some(bracket) = rest.find(']')
                {
                    let code = &rest[..bracket];
                    let message = rest[bracket + 1..].trim().trim_start_matches(':').trim();
                    let (file, ln) = if i + 1 < lines.len() {
                        parse_location(lines[i + 1])
                    } else {
                        ("unknown".to_string(), 0)
                    };
                    errors.push(json!({
                        "code": code,
                        "message": message,
                        "file": file,
                        "line": ln
                    }));
                }
            }

            Ok(json!({"errors": errors}))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::CI_CLIPPY_VIOLATIONS)
            .describe("Parse clippy warning output into structured violation records.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let log = input.get("log").and_then(|l| l.as_str()).unwrap_or("");
            let mut violations = Vec::new();
            let lines: Vec<&str> = log.lines().collect();

            for (i, line) in lines.iter().enumerate() {
                if line.trim().starts_with("warning:") {
                    let message = line
                        .trim()
                        .strip_prefix("warning: ")
                        .unwrap_or("")
                        .to_string();
                    let lint = lines
                        .iter()
                        .skip(i)
                        .take(5)
                        .find_map(|l| {
                            l.find("#[warn(").map(|pos| {
                                let rest = &l[pos + 7..];
                                rest.split(')').next().unwrap_or("").to_string()
                            })
                        })
                        .unwrap_or_default();
                    let (file, ln) = if i + 1 < lines.len() {
                        parse_location(lines[i + 1])
                    } else {
                        ("unknown".to_string(), 0)
                    };
                    violations.push(json!({
                        "lint": lint,
                        "message": message,
                        "file": file,
                        "line": ln
                    }));
                }
            }

            Ok(json!({"violations": violations}))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::CI_NEXTEST_FAILURES)
            .describe("Parse nextest output into structured test failure records.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let log = input.get("log").and_then(|l| l.as_str()).unwrap_or("");
            let mut failures = Vec::new();

            for line in log.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("FAIL") {
                    let test_name = trimmed
                        .split(']')
                        .nth(1)
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default();
                    failures.push(json!({
                        "test_name": test_name,
                        "message": "",
                        "file": ""
                    }));
                } else if trimmed.starts_with("thread '")
                    && trimmed.contains("panicked at")
                    && let Some(last) = failures.last_mut()
                {
                    let msg = trimmed
                        .split("panicked at")
                        .nth(1)
                        .map(|s| s.trim().trim_matches('\''))
                        .unwrap_or("");
                    last["message"] = Value::String(msg.to_string());
                }
            }

            Ok(json!({"failures": failures}))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::CI_DENY_VIOLATIONS)
            .describe("Parse cargo-deny output into structured license/advisory violation records.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let log = input
                .get("log")
                .or_else(|| input.get("stdout"))
                .and_then(|l| l.as_str())
                .unwrap_or("");
            let mut violations = Vec::new();

            for line in log.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("error[")
                    && let Some(bracket) = rest.find(']')
                {
                    let kind = &rest[..bracket];
                    let message = rest[bracket + 1..].trim().trim_start_matches(':').trim();
                    let crate_name = message
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("unknown")
                        .to_string();
                    violations.push(json!({
                        "kind": kind,
                        "crate_name": crate_name,
                        "message": message
                    }));
                }
            }

            Ok(json!({"violations": violations}))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::CI_DEDUPLICATE_SPANS)
            .describe("Remove duplicate diagnostic spans from errors, violations, and failures.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let mut result = serde_json::Map::new();

            for key in ["errors", "violations", "failures"] {
                if let Some(items) = input.get(key).and_then(|v| v.as_array()) {
                    let mut seen = HashSet::new();
                    let deduped: Vec<Value> = items
                        .iter()
                        .filter(|item| {
                            let file = item.get("file").and_then(|f| f.as_str()).unwrap_or("");
                            let line = item.get("line").and_then(|l| l.as_u64()).unwrap_or(0);
                            let source = item
                                .get("source")
                                .or_else(|| item.get("kind"))
                                .and_then(|s| s.as_str())
                                .unwrap_or("");
                            seen.insert(format!("{source}:{file}:{line}"))
                        })
                        .cloned()
                        .collect();
                    result.insert(key.to_string(), Value::Array(deduped));
                }
            }

            Ok(Value::Object(result))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::CI_CLASSIFY_SEVERITY)
            .describe("Rank and label diagnostics by severity: error > failure > warning > info.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let items = input
                .get("items")
                .and_then(|i| i.as_array())
                .cloned()
                .unwrap_or_default();

            let severity_order = |source: &str| -> u8 {
                match source {
                    "compile" => 0,
                    "deny" => 1,
                    "test" => 2,
                    "clippy" => 3,
                    _ => 4,
                }
            };

            let severity_label = |source: &str| -> &str {
                match source {
                    "compile" => "error",
                    "deny" => "error",
                    "test" => "failure",
                    "clippy" => "warning",
                    _ => "info",
                }
            };

            let mut ranked: Vec<Value> = items
                .into_iter()
                .map(|mut item| {
                    let source = item
                        .get("source")
                        .and_then(|s| s.as_str())
                        .unwrap_or("unknown");
                    let label = severity_label(source).to_string();
                    if let Value::Object(ref mut m) = item {
                        m.insert("severity".to_string(), Value::String(label));
                    }
                    item
                })
                .collect();

            ranked.sort_by_key(|item| {
                let source = item
                    .get("source")
                    .and_then(|s| s.as_str())
                    .unwrap_or("unknown");
                severity_order(source)
            });

            Ok(json!({"ranked": ranked}))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::CI_ATTACH_OWNERS)
            .describe("Annotate ranked diagnostics with the owning crate name via cargo metadata.")
            .risk(RiskLevel::Low)
            .side_effects(vec![SideEffect::Shell, SideEffect::Process])
            .deterministic(true),
        |input: Value| async move {
            let ranked = input
                .get("ranked")
                .and_then(|r| r.as_array())
                .cloned()
                .unwrap_or_default();

            let output = tokio::process::Command::new("cargo")
                .args(["metadata", "--no-deps", "--format-version", "1"])
                .output()
                .await;

            let crate_map: HashMap<String, String> = if let Ok(out) = output {
                let meta: Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
                meta.get("packages")
                    .and_then(|p| p.as_array())
                    .map(|pkgs| {
                        pkgs.iter()
                            .filter_map(|pkg| {
                                let name = pkg.get("name")?.as_str()?;
                                let manifest = pkg.get("manifest_path")?.as_str()?;
                                let dir = manifest.rsplit_once('/')?.0;
                                Some((dir.to_string(), name.to_string()))
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                HashMap::new()
            };

            let annotated: Vec<Value> = ranked
                .into_iter()
                .map(|mut item| {
                    let file = item.get("file").and_then(|f| f.as_str()).unwrap_or("");
                    let owner = crate_map
                        .iter()
                        .find(|(dir, _)| file.starts_with(dir.as_str()))
                        .map(|(_, name)| name.as_str())
                        .unwrap_or("unknown");
                    if let Value::Object(ref mut m) = item {
                        m.insert("crate_name".to_string(), Value::String(owner.to_string()));
                    }
                    item
                })
                .collect();

            Ok(json!({"ranked": annotated}))
        },
    );

    registry.register_metadata(
        HandlerMetadata::new(handlers::CI_SCORE_FIXABILITY)
            .describe("Score the auto-fixability ratio of ranked diagnostics.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler(handlers::CI_SCORE_FIXABILITY, |input: Value| async move {
        let ranked = input
            .get("ranked")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();

        let total = ranked.len() as f64;
        if total == 0.0 {
            return Ok(HandlerOutput::with_confidence(json!({"ranked": []}), 1.0));
        }

        let auto_fixable = ranked
            .iter()
            .filter(|item| {
                let source = item.get("source").and_then(|s| s.as_str()).unwrap_or("");
                let msg = item.get("message").and_then(|m| m.as_str()).unwrap_or("");
                source == "clippy"
                    || msg.contains("unused import")
                    || msg.contains("unused variable")
            })
            .count() as f64;

        let score = auto_fixable / total;

        Ok(HandlerOutput::with_confidence(
            json!({"ranked": ranked}),
            score as f32,
        ))
    });
}

fn parse_location(line: &str) -> (String, u64) {
    let trimmed = line.trim().strip_prefix("-->").unwrap_or(line).trim();
    // Format: "src/main.rs:10:5"
    let parts: Vec<&str> = trimmed.rsplitn(3, ':').collect();
    if parts.len() >= 3 {
        let file = parts[2].to_string();
        let ln = parts[1].parse::<u64>().unwrap_or(0);
        (file, ln)
    } else {
        (trimmed.to_string(), 0)
    }
}
