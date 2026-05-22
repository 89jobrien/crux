use chrono::{DateTime, Utc};
use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerMetadata, HandlerOutput, HandlerRegistry, RiskLevel};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::handlers;

const SLOW_STEP_MULTIPLIER: f64 = 2.0;
const TOP_TOKEN_SPEND_COUNT: usize = 3;
const BUDGET_TIGHTEN_THRESHOLD: f64 = 0.8;
const BUDGET_TIGHTEN_FACTOR: f64 = 1.1;
const FLAKY_THRESHOLD: f64 = 0.4;

pub fn register(registry: &mut HandlerRegistry) {
    register_latency_profile(registry);
    register_token_spend(registry);
    register_failure_clusters(registry);
    register_replay_cache_hits(registry);
    register_tighten_budget(registry);
    register_compress_stages(registry);
    register_tune_retry(registry);
    register_patch_schema_check(registry);
    register_replay_dry_run(registry);
}

fn register_latency_profile(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::ANALYSIS_LATENCY_PROFILE)
            .describe("Profile step latencies and identify slow outliers.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let steps = input
                .get("steps")
                .and_then(|s| s.as_array())
                .cloned()
                .unwrap_or_default();

            let mut durations: Vec<(String, f64)> = Vec::new();
            for step in &steps {
                let name = step
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown");
                let started = step
                    .get("started_at")
                    .and_then(|s| s.as_str())
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok());
                let completed = step
                    .get("completed_at")
                    .and_then(|s| s.as_str())
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok());
                if let (Some(s), Some(c)) = (started, completed) {
                    let ms = (c - s).num_milliseconds().max(0) as f64;
                    durations.push((name.to_string(), ms));
                }
            }

            if durations.is_empty() {
                return Ok(json!({"slow_steps": [], "median_ms": 0}));
            }

            let mut sorted: Vec<f64> = durations.iter().map(|(_, d)| *d).collect();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mid = sorted.len() / 2;
            let median = if sorted.len().is_multiple_of(2) {
                (sorted[mid - 1] + sorted[mid]) / 2.0
            } else {
                sorted[mid]
            };

            let slow: Vec<Value> = durations
                .iter()
                .filter(|(_, d)| *d > median * SLOW_STEP_MULTIPLIER)
                .map(|(name, d)| {
                    json!({
                        "name": name,
                        "duration_ms": *d,
                        "ratio_to_median": if median > 0.0 { *d / median } else { 0.0 }
                    })
                })
                .collect();

            Ok(json!({"slow_steps": slow, "median_ms": median}))
        },
    );
}

fn register_token_spend(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::ANALYSIS_TOKEN_SPEND)
            .describe("Tally token spend per step and identify top consumers.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let steps = input
                .get("steps")
                .and_then(|s| s.as_array())
                .cloned()
                .unwrap_or_default();

            let mut by_step: Vec<(String, u64)> = Vec::new();
            for step in &steps {
                let name = step
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let tokens = step
                    .pointer("/output/metadata/tokens")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0);
                by_step.push((name, tokens));
            }

            by_step.sort_by(|a, b| b.1.cmp(&a.1));
            let total: u64 = by_step.iter().map(|(_, t)| *t).sum();
            let top3: Vec<&str> = by_step
                .iter()
                .take(TOP_TOKEN_SPEND_COUNT)
                .map(|(n, _)| n.as_str())
                .collect();
            let items: Vec<Value> = by_step
                .iter()
                .map(|(n, t)| json!({"name": n, "tokens": t}))
                .collect();

            Ok(json!({"by_step": items, "total": total, "top3": top3}))
        },
    );
}

fn register_failure_clusters(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::ANALYSIS_FAILURE_CLUSTERS)
            .describe("Cluster failed steps by error kind.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let steps = input
                .get("steps")
                .and_then(|s| s.as_array())
                .cloned()
                .unwrap_or_default();

            let mut clusters: HashMap<String, Vec<String>> = HashMap::new();
            for step in &steps {
                let status = step.get("status").and_then(|s| s.as_str()).unwrap_or("");
                if status != "failed" {
                    continue;
                }
                let kind = step
                    .pointer("/error/kind")
                    .and_then(|k| k.as_str())
                    .unwrap_or("Unknown")
                    .to_string();
                let name = step
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                clusters.entry(kind).or_default().push(name);
            }

            let items: Vec<Value> = clusters
                .into_iter()
                .map(|(kind, names)| {
                    json!({"kind": kind, "count": names.len(), "step_names": names})
                })
                .collect();

            Ok(json!({"clusters": items}))
        },
    );
}

fn register_replay_cache_hits(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::ANALYSIS_REPLAY_CACHE_HITS)
            .describe("Compute per-step replay cache hit/miss ratios.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let steps = input
                .get("steps")
                .and_then(|s| s.as_array())
                .cloned()
                .unwrap_or_default();

            let mut stats: HashMap<String, (u64, u64)> = HashMap::new();
            for step in &steps {
                let name = step
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let hit = step
                    .get("cache_hit")
                    .and_then(|h| h.as_bool())
                    .unwrap_or(false);
                let entry = stats.entry(name).or_insert((0, 0));
                if hit {
                    entry.0 += 1;
                } else {
                    entry.1 += 1;
                }
            }

            let items: Vec<Value> = stats
                .into_iter()
                .map(|(name, (hits, misses))| {
                    let total = hits + misses;
                    json!({
                        "name": name,
                        "hits": hits,
                        "misses": misses,
                        "ratio": if total > 0 { hits as f64 / total as f64 } else { 0.0 }
                    })
                })
                .collect();

            Ok(json!({"by_step": items}))
        },
    );
}

fn register_tighten_budget(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::ANALYSIS_TIGHTEN_BUDGET)
            .describe("Suggest a tighter token budget based on actual spend ratio.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler(
        handlers::ANALYSIS_TIGHTEN_BUDGET,
        |input: Value| async move {
            let spend = input
                .pointer("/token_spend/total")
                .and_then(|t| t.as_f64())
                .unwrap_or(0.0);
            let budget = input
                .pointer("/budget/tokens")
                .and_then(|t| t.as_f64())
                .unwrap_or(1.0);

            let ratio = if budget > 0.0 { spend / budget } else { 0.0 };

            if ratio > BUDGET_TIGHTEN_THRESHOLD {
                let suggested = (spend * BUDGET_TIGHTEN_FACTOR).ceil() as u64;
                Ok(HandlerOutput::with_confidence(
                    json!({
                        "suggestion": {
                            "current_budget": budget,
                            "actual_spend": spend,
                            "ratio": ratio,
                            "suggested_budget": suggested
                        }
                    }),
                    (ratio as f32).min(1.0),
                ))
            } else {
                Ok(HandlerOutput::with_confidence(
                    json!({"ratio": ratio}),
                    (ratio as f32).min(1.0),
                ))
            }
        },
    );
}

fn register_compress_stages(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::ANALYSIS_COMPRESS_STAGES)
            .describe("Suggest pipeline stage compression for high token-fraction steps.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let by_step = input
                .pointer("/token_spend/by_step")
                .and_then(|s| s.as_array())
                .cloned()
                .unwrap_or_default();
            let total = input
                .pointer("/token_spend/total")
                .and_then(|t| t.as_f64())
                .unwrap_or(1.0);

            let suggestions: Vec<Value> = by_step
                .iter()
                .filter_map(|step| {
                    let tokens = step.get("tokens").and_then(|t| t.as_f64()).unwrap_or(0.0);
                    let fraction = if total > 0.0 { tokens / total } else { 0.0 };
                    if fraction > FLAKY_THRESHOLD {
                        Some(json!({
                            "stage": step.get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("unknown"),
                            "reason": format!(
                                "consumes {:.0}% of total tokens",
                                fraction * 100.0
                            ),
                            "action": "collapse stages or use cheaper model"
                        }))
                    } else {
                        None
                    }
                })
                .collect();

            Ok(json!({"suggestions": suggestions}))
        },
    );
}

fn register_tune_retry(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::ANALYSIS_TUNE_RETRY)
            .describe("Suggest retry parameters for steps with recurring failures.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let clusters = input
                .pointer("/failure_clusters/clusters")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();

            let suggestions: Vec<Value> = clusters
                .iter()
                .filter_map(|cluster| {
                    let count = cluster.get("count").and_then(|c| c.as_u64()).unwrap_or(0);
                    if count <= 2 {
                        return None;
                    }
                    let names = cluster
                        .get("step_names")
                        .and_then(|n| n.as_array())
                        .cloned()
                        .unwrap_or_default();
                    Some(
                        names
                            .iter()
                            .filter_map(|n| n.as_str())
                            .map(|name| {
                                json!({
                                    "step_name": name,
                                    "retry_count": 3,
                                    "backoff_ms": 1000
                                })
                            })
                            .collect::<Vec<Value>>(),
                    )
                })
                .flatten()
                .collect();

            Ok(json!({"suggestions": suggestions}))
        },
    );
}

fn register_patch_schema_check(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::ANALYSIS_PATCH_SCHEMA_CHECK)
            .describe("Validate a YAML patch against the pipeline schema.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let patch = input.get("patch").and_then(|p| p.as_str()).unwrap_or("");

            if patch.is_empty() {
                return Ok(json!({"valid": false, "errors": ["empty patch"]}));
            }

            match serde_yaml::from_str::<Value>(patch) {
                Ok(_) => Ok(json!({"valid": true, "errors": []})),
                Err(e) => Ok(json!({"valid": false, "errors": [e.to_string()]})),
            }
        },
    );
}

fn register_replay_dry_run(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::ANALYSIS_REPLAY_DRY_RUN)
            .describe("Dry-run a replay with a patch to detect step ordinal mismatches.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let trace_path = input
                .get("trace_path")
                .and_then(|p| p.as_str())
                .unwrap_or("");
            let patch = input.get("patch").and_then(|p| p.as_str()).unwrap_or("");

            if trace_path.is_empty() || patch.is_empty() {
                return Ok(json!({"ok": false, "mismatches": ["missing trace_path or patch"]}));
            }

            // Reject paths with suspicious characters (argument injection guard)
            if trace_path.starts_with('-') || trace_path.contains('\0') {
                return Err(CruxErr::step_failed(
                    "analysis::replay_dry_run",
                    format!("invalid trace_path: {trace_path}"),
                ));
            }

            let tmp =
                std::env::temp_dir().join(format!("crux-replay-patch-{}.yaml", std::process::id()));
            tokio::fs::write(&tmp, patch).await.map_err(|e| {
                CruxErr::step_failed("analysis::replay_dry_run", format!("write temp: {e}"))
            })?;

            let output = tokio::process::Command::new("crux")
                .args([
                    "replay",
                    "--lenient",
                    "--",
                    trace_path,
                    tmp.to_str().unwrap_or(""),
                ])
                .output()
                .await
                .map_err(|e| {
                    CruxErr::step_failed("analysis::replay_dry_run", format!("exec: {e}"))
                })?;

            let ok = output.status.success();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let mismatches: Vec<String> = if ok {
                vec![]
            } else {
                stderr.lines().map(|l| l.to_string()).collect()
            };

            Ok(json!({"ok": ok, "mismatches": mismatches}))
        },
    );
}
