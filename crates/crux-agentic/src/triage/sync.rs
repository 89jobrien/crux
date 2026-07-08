//! Handlers for matching todos to issues, identifying untracked items,
//! plan-to-commit matching, and status mismatch detection.

use crux_script::{HandlerMetadata, HandlerOutput, HandlerRegistry, RiskLevel};
use serde_json::{Value, json};

use crate::handlers;

use super::util::edit_distance;

// Todo-issue fuzzy match threshold
const TODO_ISSUE_MATCH_THRESHOLD: f64 = 0.5;

// Minimum keyword length for plan-to-commit matching
const MIN_KEYWORD_LENGTH: usize = 4;

pub(super) fn register(registry: &mut HandlerRegistry) {
    register_match_todos_to_issues(registry);
    register_identify_untracked(registry);
    register_match_plans_to_commits(registry);
    register_detect_status_mismatch(registry);
}

fn register_match_todos_to_issues(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::TRIAGE_MATCH_TODOS_TO_ISSUES)
            .describe("Fuzzy-match todo items to GitHub issues by title edit distance.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let matches_arr = input
                .get("matches")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let issues: Vec<Value> = input
                .get("fetch_issues")
                .and_then(|v| v.get("output"))
                .and_then(|v| v.as_str())
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();

            let matched: Vec<Value> = matches_arr
                .iter()
                .map(|todo| {
                    let text = todo
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let issue = issues.iter().find(|iss| {
                        let title = iss
                            .get("title")
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .to_lowercase();
                        let dist = edit_distance(&text, &title);
                        let max_len = text.len().max(title.len()) as f64;
                        max_len > 0.0 && (dist as f64 / max_len) < TODO_ISSUE_MATCH_THRESHOLD
                    });
                    let mut out = todo.clone();
                    if let Some(iss) = issue
                        && let Value::Object(ref mut m) = out
                    {
                        m.insert("matched_issue".to_string(), iss.clone());
                    }
                    out
                })
                .collect();

            Ok(json!({"matched": matched}))
        },
    );
}

fn register_identify_untracked(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_IDENTIFY_UNTRACKED)
            .describe("Identify todos with no matching issue and emit a coverage confidence score.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler(
        handlers::TRIAGE_IDENTIFY_UNTRACKED,
        |input: Value| async move {
            let matched = input
                .get("matched")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let untracked: Vec<Value> = matched
                .iter()
                .filter(|t| t.get("matched_issue").is_none())
                .cloned()
                .collect();

            let total = matched.len() as f64;
            let untracked_count = untracked.len() as f64;
            let confidence: f32 = if total == 0.0 {
                1.0
            } else {
                (1.0 - untracked_count / total) as f32
            };

            Ok(HandlerOutput::with_confidence(
                json!({"untracked": untracked, "total": total as u64, "untracked_count": untracked_count as u64}),
                confidence,
            ))
        },
    );
}

fn register_match_plans_to_commits(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::TRIAGE_MATCH_PLANS_TO_COMMITS)
            .describe("Check whether each plan title has a keyword match in the recent commit log.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let plans = input
                .get("frontmatter")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let commit_text = input
                .pointer("/recent_commits/output")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let matched: Vec<Value> = plans
                .iter()
                .map(|plan| {
                    let title = plan
                        .get("title")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_lowercase();
                    // Check if any commit message word-matches the plan title
                    let keywords: Vec<&str> = title.split_whitespace().collect();
                    let has_commit = keywords.iter().any(|kw| {
                        kw.len() > MIN_KEYWORD_LENGTH && commit_text.to_lowercase().contains(*kw)
                    });
                    let mut out = plan.clone();
                    if let Value::Object(ref mut m) = out {
                        m.insert("has_matching_commit".to_string(), Value::Bool(has_commit));
                    }
                    out
                })
                .collect();

            Ok(json!({"plans": matched}))
        },
    );
}

fn register_detect_status_mismatch(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_DETECT_STATUS_MISMATCH)
            .describe("Flag plans whose status contradicts their commit coverage.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler(
        handlers::TRIAGE_DETECT_STATUS_MISMATCH,
        |input: Value| async move {
            let plans = input
                .get("plans")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut mismatches = Vec::new();
            for plan in &plans {
                let status = plan
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("open");
                let has_commit = plan
                    .get("has_matching_commit")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if status == "done" && !has_commit {
                    mismatches.push(
                        json!({"plan": plan, "mismatch": "marked done but no matching commit"}),
                    );
                } else if status == "open" && has_commit {
                    mismatches.push(
                        json!({"plan": plan, "mismatch": "has commits but still marked open"}),
                    );
                }
            }

            let confidence: f32 = if plans.is_empty() {
                1.0
            } else {
                (1.0 - mismatches.len() as f64 / plans.len() as f64) as f32
            };

            Ok(HandlerOutput::with_confidence(
                json!({"mismatches": mismatches, "total_plans": plans.len()}),
                confidence,
            ))
        },
    );
}
