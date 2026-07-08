//! Handlers for todo parsing, urgency scoring, deduplication, grouping, and gate merging.

use chrono::{DateTime, Utc};
use crux_script::{HandlerMetadata, HandlerRegistry, RiskLevel};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::handlers;

use super::util::edit_distance;

/// Maximum normalized edit distance (0.0-1.0) for two titles to be
/// considered duplicates. Lower = stricter matching.
const DEDUP_DISTANCE_THRESHOLD: f64 = 0.4;

// Priority weights for urgency scoring
const PRIORITY_CRITICAL_WEIGHT: f64 = 4.0;
const PRIORITY_HIGH_WEIGHT: f64 = 3.0;
const PRIORITY_MEDIUM_WEIGHT: f64 = 2.0;
const PRIORITY_LOW_WEIGHT: f64 = 1.0;

pub(super) fn register(registry: &mut HandlerRegistry) {
    register_parse_repo_tags(registry);
    register_score_urgency(registry);
    register_deduplicate_intent(registry);
    register_group_by_repo(registry);
    register_merge_results(registry);
}

fn register_parse_repo_tags(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::TRIAGE_PARSE_REPO_TAGS)
            .describe("Extract repo tag from todo metadata and attach it to each todo item.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let todos = input
                .get("todos")
                .and_then(|t| t.as_array())
                .cloned()
                .unwrap_or_default();

            let tagged: Vec<Value> = todos
                .into_iter()
                .map(|mut todo| {
                    let repo = todo
                        .pointer("/metadata/repo")
                        .and_then(|r| r.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    if let Value::Object(ref mut m) = todo {
                        m.insert("repo".to_string(), Value::String(repo));
                    }
                    todo
                })
                .collect();

            Ok(json!({"todos": tagged}))
        },
    );
}

fn register_score_urgency(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_SCORE_URGENCY)
            .describe("Score and sort todos by urgency using priority weight and age in days.")
            .risk(RiskLevel::Low)
            .deterministic(false),
    );
    registry.handler_value(handlers::TRIAGE_SCORE_URGENCY, |input: Value| async move {
        let todos = input
            .get("todos")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();

        let now = Utc::now();
        let mut scored: Vec<(f64, Value)> = todos
            .into_iter()
            .map(|mut todo| {
                let priority = todo
                    .get("priority")
                    .and_then(|p| p.as_str())
                    .unwrap_or("medium");
                let weight: f64 = match priority {
                    "critical" => PRIORITY_CRITICAL_WEIGHT,
                    "high" => PRIORITY_HIGH_WEIGHT,
                    "medium" => PRIORITY_MEDIUM_WEIGHT,
                    "low" => PRIORITY_LOW_WEIGHT,
                    _ => PRIORITY_LOW_WEIGHT,
                };
                let created = todo
                    .get("created_at")
                    .and_then(|c| c.as_str())
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok());
                let age_days = created.map(|c| (now - c).num_days() as f64).unwrap_or(0.0);
                let score = age_days * weight;

                if let Value::Object(ref mut m) = todo {
                    m.insert("urgency_score".to_string(), json!(score));
                }
                (score, todo)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let result: Vec<Value> = scored.into_iter().map(|(_, t)| t).collect();
        Ok(json!({"todos": result}))
    });
}

fn register_deduplicate_intent(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::TRIAGE_DEDUPLICATE_INTENT)
            .describe("Group todos with similar titles using normalized edit distance.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let todos = input
                .get("todos")
                .and_then(|t| t.as_array())
                .cloned()
                .unwrap_or_default();

            let mut groups: Vec<(String, Vec<Value>)> = Vec::new();

            for todo in todos {
                let title = todo
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_lowercase();

                let matched = groups.iter_mut().find(|(canonical, _)| {
                    let dist = edit_distance(canonical, &title);
                    let max_len = canonical.len().max(title.len()) as f64;
                    if max_len == 0.0 {
                        return true;
                    }
                    (dist as f64 / max_len) < DEDUP_DISTANCE_THRESHOLD
                });

                if let Some((_, members)) = matched {
                    members.push(todo);
                } else {
                    groups.push((title, vec![todo]));
                }
            }

            let result: Vec<Value> = groups
                .into_iter()
                .map(|(canonical, members)| {
                    let ids: Vec<Value> = members
                        .iter()
                        .filter_map(|m| m.get("id").cloned())
                        .collect();
                    json!({
                        "canonical": canonical,
                        "duplicates": ids,
                        "items": members
                    })
                })
                .collect();

            Ok(json!({"groups": result}))
        },
    );
}

fn register_group_by_repo(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_GROUP_BY_REPO)
            .describe("Group todos into a map keyed by repo name.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler_value(handlers::TRIAGE_GROUP_BY_REPO, |input: Value| async move {
        let todos = input
            .get("todos")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();

        let mut repos: HashMap<String, Vec<Value>> = HashMap::new();
        for todo in todos {
            let repo = todo
                .get("repo")
                .and_then(|r| r.as_str())
                .unwrap_or("unknown")
                .to_string();
            repos.entry(repo).or_default().push(todo);
        }

        let map: serde_json::Map<String, Value> = repos
            .into_iter()
            .map(|(k, v)| (k, Value::Array(v)))
            .collect();

        Ok(json!({"repos": map}))
    });
}

fn register_merge_results(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_MERGE_RESULTS)
            .describe("Aggregate gate pass/fail results into a single summary.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler_value(handlers::TRIAGE_MERGE_RESULTS, |input: Value| async move {
        let keys = [
            "cargo_fmt",
            "cargo_clippy",
            "cargo_test",
            "git_status",
            "doob_stale",
        ];
        let mut passed = 0u64;
        let mut failed = 0u64;
        let mut summary = Vec::new();

        for key in &keys {
            if let Some(v) = input.get(key) {
                let output = v
                    .get("output")
                    .or_else(|| v.get("stdout"))
                    .and_then(|o| o.as_str())
                    .unwrap_or("");
                let ok = output.contains("error") || output.contains("FAILED");
                if ok {
                    failed += 1;
                    summary.push(json!({"gate": key, "status": "failed"}));
                } else {
                    passed += 1;
                    summary.push(json!({"gate": key, "status": "passed"}));
                }
            }
        }

        Ok(json!({"passed": passed, "failed": failed, "gates": summary}))
    });
}
