use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerMetadata, HandlerOutput, HandlerRegistry, RiskLevel, SideEffect};
use serde_json::{Value, json};

use crate::handlers;

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::REVIEW_ARCH_BOUNDARY_CHECK)
            .describe("Scan staged files for domain-layer imports of adapter/infra modules.")
            .risk(RiskLevel::Low)
            .side_effects(vec![SideEffect::Shell])
            .deterministic(true),
        |input: Value| async move {
            let files = input
                .get("files")
                .and_then(|f| f.as_array())
                .cloned()
                .unwrap_or_default();

            let file_list: Vec<&str> = files
                .iter()
                .filter_map(|f| f.as_str())
                .filter(|f| !f.starts_with('-') && !f.contains('\0'))
                .collect();

            if file_list.is_empty() {
                return Ok(json!({"violations": []}));
            }

            let pattern = r"use\s+(crate::adapters|infra::|adapter::)";
            let mut violations = Vec::new();

            for file in &file_list {
                let output = tokio::process::Command::new("rg")
                    .args(["--no-heading", "-n", "--", pattern, file])
                    .output()
                    .await;

                if let Ok(out) = output
                    && out.status.success()
                {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    for line in stdout.lines() {
                        violations.push(json!({
                            "file": file,
                            "imports": line.trim(),
                            "violation": "domain imports adapter/infra"
                        }));
                    }
                }
            }

            Ok(json!({"violations": violations}))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::REVIEW_NORMALIZE_FINDINGS)
            .describe("Merge clippy, arch, and coverage findings into a unified list.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let mut findings = Vec::new();

            if let Some(violations) = input
                .pointer("/clippy/violations")
                .and_then(|v| v.as_array())
            {
                for v in violations {
                    findings.push(json!({
                        "source": "clippy",
                        "file": v.get("file").unwrap_or(&Value::Null),
                        "line": v.get("line").unwrap_or(&Value::Null),
                        "message": v.get("message")
                            .or_else(|| v.get("lint"))
                            .unwrap_or(&Value::Null),
                        "severity": "warning"
                    }));
                }
            }

            if let Some(violations) = input.pointer("/arch/violations").and_then(|v| v.as_array()) {
                for v in violations {
                    findings.push(json!({
                        "source": "arch",
                        "file": v.get("file").unwrap_or(&Value::Null),
                        "line": Value::Null,
                        "message": v.get("violation").unwrap_or(&Value::Null),
                        "severity": "error"
                    }));
                }
            }

            if let Some(uncovered) = input
                .pointer("/coverage/uncovered")
                .and_then(|v| v.as_array())
            {
                for u in uncovered {
                    let loc = u.as_str().unwrap_or("");
                    let (file, line) = loc.rsplit_once(':').unwrap_or((loc, "0"));
                    findings.push(json!({
                        "source": "coverage",
                        "file": file,
                        "line": line.parse::<u64>().unwrap_or(0),
                        "message": "uncovered code path",
                        "severity": "info"
                    }));
                }
            }

            Ok(json!({"findings": findings}))
        },
    );

    registry.register_metadata(
        HandlerMetadata::new(handlers::REVIEW_APPLY_SEVERITY)
            .describe("Tag each finding with a severity tier based on its source.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler_value(handlers::REVIEW_APPLY_SEVERITY, |input: Value| async move {
        let findings = input
            .get("findings")
            .and_then(|f| f.as_array())
            .cloned()
            .unwrap_or_default();

        let tiered: Vec<Value> = findings
            .into_iter()
            .map(|mut f| {
                let source = f.get("source").and_then(|s| s.as_str()).unwrap_or("");
                let tier = match source {
                    "compile" | "arch" | "deny" => "blocking",
                    "clippy" | "test" => "suggestion",
                    _ => "observation",
                };
                if let Value::Object(ref mut m) = f {
                    m.insert("tier".to_string(), Value::String(tier.to_string()));
                }
                f
            })
            .collect();

        Ok(json!({"findings": tiered}))
    });

    registry.register_metadata(
        HandlerMetadata::new(handlers::REVIEW_COMPUTE_SCORE)
            .describe("Compute a 0–1 review score from the ratio of blocking findings.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler(handlers::REVIEW_COMPUTE_SCORE, |input: Value| async move {
        let findings = input
            .get("findings")
            .and_then(|f| f.as_array())
            .cloned()
            .unwrap_or_default();

        let total = findings.len() as f64;
        if total == 0.0 {
            return Ok(HandlerOutput::with_confidence(
                json!({"score": 1.0, "blocking_count": 0}),
                1.0,
            ));
        }

        let blocking = findings
            .iter()
            .filter(|f| f.get("tier").and_then(|t| t.as_str()) == Some("blocking"))
            .count() as f64;

        let score = 1.0 - (blocking / total);

        Ok(HandlerOutput::with_confidence(
            json!({
                "score": score,
                "blocking_count": blocking as u64,
                "total_findings": total as u64
            }),
            score as f32,
        ))
    });

    registry.register_metadata(
        HandlerMetadata::new(handlers::REVIEW_APPROVE)
            .describe("Approve a GitHub PR via the gh CLI.")
            .risk(RiskLevel::Medium)
            .side_effects(vec![SideEffect::Shell])
            .deterministic(false),
    );
    registry.handler_value(handlers::REVIEW_APPROVE, |input: Value| async move {
        let pr_number = input
            .get("args")
            .and_then(|a| a.get("pr"))
            .and_then(|p| p.as_str())
            .or_else(|| input.get("pr").and_then(|p| p.as_str()));

        // Validate PR number is numeric or a valid URL-safe string
        if let Some(pr) = pr_number
            && (pr.starts_with('-') || pr.contains('\0') || pr.contains(' '))
        {
            return Err(CruxErr::step_failed(
                "review::approve",
                format!("invalid PR identifier: {pr}"),
            ));
        }

        let mut cmd = tokio::process::Command::new("gh");
        cmd.args(["pr", "review", "--approve"]);
        if let Some(pr) = pr_number {
            cmd.arg(pr);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| CruxErr::step_failed("review::approve", format!("exec: {e}")))?;

        if output.status.success() {
            Ok(json!({"approved": true}))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(CruxErr::step_failed(
                "review::approve",
                format!("gh pr review failed: {stderr}"),
            ))
        }
    });

    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::REVIEW_DETECT_ANTIPATTERNS)
            .describe("Detect bare unwrap, panic, unsafe, and other antipatterns in diff hunks.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let files = input
                .get("files")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            // (needle, message, severity)
            let patterns: &[(&str, &str, &str)] = &[
                (".unwrap()", "bare unwrap — use expect() or ?", "suggestion"),
                (
                    "panic!(",
                    "bare panic — consider returning an error",
                    "suggestion",
                ),
                ("TODO", "TODO without issue reference", "nitpick"),
                ("FIXME", "FIXME without issue reference", "nitpick"),
                (
                    "unsafe {",
                    "unsafe block — needs safety comment",
                    "blocking",
                ),
                (
                    "println!(",
                    "println! in non-test code — use tracing",
                    "nitpick",
                ),
                (
                    "eprintln!(",
                    "eprintln! in non-test code — use tracing",
                    "nitpick",
                ),
            ];

            let mut findings = Vec::new();
            for file_entry in &files {
                let file = file_entry
                    .get("file")
                    .and_then(|f| f.as_str())
                    .unwrap_or("");
                let hunks = file_entry
                    .get("hunks")
                    .and_then(|h| h.as_array())
                    .cloned()
                    .unwrap_or_default();
                for hunk in &hunks {
                    let lines = hunk
                        .get("lines")
                        .and_then(|l| l.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let new_start = hunk.get("new_start").and_then(|n| n.as_u64()).unwrap_or(0);
                    for (i, line) in lines.iter().enumerate() {
                        let text = line.as_str().unwrap_or("");
                        if !text.starts_with('+') {
                            continue; // only added lines
                        }
                        for (needle, message, severity) in patterns {
                            if text.contains(needle) {
                                findings.push(json!({
                                    "file": file,
                                    "line": new_start + i as u64,
                                    "message": message,
                                    "severity": severity,
                                    "source": "antipattern",
                                }));
                            }
                        }
                    }
                }
            }

            Ok(json!({"findings": findings}))
        },
    );

    registry.register_metadata(
        HandlerMetadata::new(handlers::REVIEW_GROUP_BY_FILE)
            .describe("Group findings by file path into a map.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler_value(handlers::REVIEW_GROUP_BY_FILE, |input: Value| async move {
        let findings = input
            .get("findings")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut by_file: std::collections::HashMap<String, Vec<Value>> =
            std::collections::HashMap::new();
        for f in findings {
            let file = f
                .get("file")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            by_file.entry(file).or_default().push(f);
        }

        let map: serde_json::Map<String, Value> = by_file
            .into_iter()
            .map(|(k, v)| (k, Value::Array(v)))
            .collect();
        Ok(Value::Object(map))
    });

    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::REVIEW_COMPOSE_DAILY_NOTE)
            .describe("Compose a dated markdown daily note from categorized commit items.")
            .risk(RiskLevel::Low)
            .deterministic(false),
        |input: Value| async move {
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let mut sections = vec![format!("# {today}\n")];

            for cat in &["feat", "fix", "chore", "docs", "refactor", "test", "other"] {
                if let Some(items) = input.get(cat).and_then(|v| v.as_array())
                    && !items.is_empty()
                {
                    sections.push(format!("## {cat}"));
                    for item in items {
                        let line = item.get("line").and_then(|l| l.as_str()).unwrap_or("");
                        sections.push(format!("- {line}"));
                    }
                    sections.push(String::new());
                }
            }

            Ok(json!({"content": sections.join("\n"), "date": today}))
        },
    );
}
