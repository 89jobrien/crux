use chrono::{DateTime, Utc};
use crux_script::{HandlerMetadata, HandlerOutput, HandlerRegistry, RiskLevel};
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::handlers;

/// Maximum normalized edit distance (0.0-1.0) for two titles to be
/// considered duplicates. Lower = stricter matching.
const DEDUP_DISTANCE_THRESHOLD: f64 = 0.4;
const MIN_KEYWORD_LENGTH: usize = 4;

// Priority weights for urgency scoring
const PRIORITY_CRITICAL_WEIGHT: f64 = 4.0;
const PRIORITY_HIGH_WEIGHT: f64 = 3.0;
const PRIORITY_MEDIUM_WEIGHT: f64 = 2.0;
const PRIORITY_LOW_WEIGHT: f64 = 1.0;

// Confidence scores for secret-chain classification
const CONFIDENCE_BROKEN_SECRETS: f32 = 0.1;
const CONFIDENCE_DIRENV_UNLOADED: f32 = 0.3;
const CONFIDENCE_KEY_MISSING: f32 = 0.6;
const CONFIDENCE_HEALTHY_SECRETS: f32 = 0.95;

// Hook overhead latency ceiling (ms) for confidence degradation
const MAX_HOOK_OVERHEAD_MS: f64 = 5000.0;

// Branch cleanup confidence thresholds
const CONFIDENCE_ORPHANED_WORKTREES: f32 = 0.3;
const MANY_BRANCHES_THRESHOLD: usize = 5;
const CONFIDENCE_MANY_BRANCHES: f32 = 0.6;
const CONFIDENCE_CLEAN_STATE: f32 = 0.9;

// Todo-issue fuzzy match threshold
const TODO_ISSUE_MATCH_THRESHOLD: f64 = 0.5;

pub fn register(registry: &mut HandlerRegistry) {
    register_parse_repo_tags(registry);
    register_score_urgency(registry);
    register_deduplicate_intent(registry);
    register_group_by_repo(registry);
    register_merge_results(registry);
    register_parse_env_probe(registry);
    register_classify_severity(registry);
    register_suggest_remediation(registry);
    register_correlate_failures(registry);
    register_measure_overhead(registry);
    register_detect_orphaned_worktrees(registry);
    register_build_cleanup_plan(registry);
    register_match_todos_to_issues(registry);
    register_identify_untracked(registry);
    register_match_plans_to_commits(registry);
    register_detect_status_mismatch(registry);
    register_categorize_commits(registry);
    register_classify_true_false(registry);
    register_generate_allowlist_entries(registry);
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

fn register_parse_env_probe(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_PARSE_ENV_PROBE)
            .describe("Parse 1Password, direnv, and dotenvx probe outputs into a findings list.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler_value(handlers::TRIAGE_PARSE_ENV_PROBE, |input: Value| async move {
        let mut findings = Vec::new();

        let op_output = input
            .pointer("/check_op_auth/output")
            .or_else(|| input.pointer("/probe_chain/check_op_auth/output"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if op_output.is_empty() || op_output.contains("error") || op_output.contains("FAILED") {
            findings.push(json!({"component": "1password", "status": "broken",
                "detail": "op account list returned no accounts or an error"}));
        } else {
            findings.push(json!({"component": "1password", "status": "ok"}));
        }

        let direnv_output = input
            .pointer("/check_direnv/output")
            .or_else(|| input.pointer("/probe_chain/check_direnv/output"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if direnv_output.contains("not loaded") || direnv_output.is_empty() {
            findings.push(json!({"component": "direnv", "status": "unloaded",
                "detail": "direnv is not active in this shell"}));
        } else {
            findings.push(json!({"component": "direnv", "status": "ok"}));
        }

        let dotenvx_output = input
            .pointer("/check_dotenvx/output")
            .or_else(|| input.pointer("/probe_chain/check_dotenvx/output"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let key_present = dotenvx_output.contains("DOTENV_PRIVATE_KEY=set");
        findings.push(json!({
            "component": "dotenvx",
            "status": if key_present { "ok" } else { "missing_key" },
            "detail": if key_present { "private key present" } else { "DOTENV_PRIVATE_KEY not set" },
        }));

        Ok(json!({"findings": findings}))
    });
}

fn register_classify_severity(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_CLASSIFY_SEVERITY)
            .describe("Classify secret-chain health and emit a confidence score.")
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler(handlers::TRIAGE_CLASSIFY_SEVERITY, |input: Value| async move {
        let findings = input
            .get("findings")
            .and_then(|f| f.as_array())
            .cloned()
            .unwrap_or_default();

        // Order: 1password > direnv > dotenvx key
        let broken = findings.iter().filter(|f| {
            f.get("status").and_then(|s| s.as_str()) == Some("broken")
        }).count();
        let unloaded = findings.iter().filter(|f| {
            f.get("status").and_then(|s| s.as_str()) == Some("unloaded")
        }).count();
        let missing = findings.iter().filter(|f| {
            f.get("status").and_then(|s| s.as_str()) == Some("missing_key")
        }).count();

        let confidence: f32 = match (broken, unloaded, missing) {
            (b, _, _) if b > 0 => CONFIDENCE_BROKEN_SECRETS,
            (_, u, _) if u > 0 => CONFIDENCE_DIRENV_UNLOADED,
            (_, _, m) if m > 0 => CONFIDENCE_KEY_MISSING,
            _ => CONFIDENCE_HEALTHY_SECRETS,
        };

        Ok(HandlerOutput::with_confidence(
            json!({"findings": findings, "broken": broken, "unloaded": unloaded, "missing": missing}),
            confidence,
        ))
    });
}

fn register_suggest_remediation(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::TRIAGE_SUGGEST_REMEDIATION)
            .describe("Suggest fix commands for broken 1Password, direnv, or dotenvx components.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let mut fixes = Vec::new();

            if input.get("broken").and_then(|v| v.as_u64()).unwrap_or(0) > 0 {
                fixes.push(json!({
                    "component": "1password",
                    "fix": "Open 1Password and unlock it, then retry: op account list"
                }));
            }
            if input.get("unloaded").and_then(|v| v.as_u64()).unwrap_or(0) > 0 {
                fixes.push(json!({
                    "component": "direnv",
                    "fix": "cd $HOME/dev && direnv allow"
                }));
            }
            if input.get("missing").and_then(|v| v.as_u64()).unwrap_or(0) > 0 {
                fixes.push(json!({
                    "component": "dotenvx",
                    "fix": "export DOTENV_PRIVATE_KEY=$(op read 'op://Personal/nihl7o2bojy53zy4aqtr7txyqi/password')"
                }));
            }
            if fixes.is_empty() {
                fixes.push(json!({"component": "all", "fix": "Chain is healthy — no action needed"}));
            }

            Ok(json!({"fixes": fixes}))
        },
    );
}

fn register_correlate_failures(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::TRIAGE_CORRELATE_FAILURES)
            .describe("Correlate hook names against recent failure text to identify culprits.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let hooks = input
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let failure_text = input
                .pointer("/recent_failures/output")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let correlated: Vec<Value> = hooks
                .iter()
                .filter_map(|h| {
                    let name = h.as_str().unwrap_or("");
                    if failure_text.contains(name) {
                        Some(json!({"hook": name, "has_failures": true}))
                    } else {
                        None
                    }
                })
                .collect();

            Ok(json!({"correlated_failures": correlated, "total_hooks": hooks.len()}))
        },
    );
}

fn register_measure_overhead(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_MEASURE_OVERHEAD)
            .describe(
                "Compute p50/p95 latency from hook duration samples and emit a confidence score.",
            )
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler(
        handlers::TRIAGE_MEASURE_OVERHEAD,
        |input: Value| async move {
            let items = input
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let durations: Vec<f64> = items
                .iter()
                .filter_map(|item| {
                    item.get("duration_ms")
                        .or_else(|| item.get("elapsed_ms"))
                        .and_then(|v| v.as_f64())
                })
                .collect();

            let (p50, p95) = if durations.is_empty() {
                (0.0, 0.0)
            } else {
                let mut sorted = durations.clone();
                sorted.sort_by(|a, b| a.total_cmp(b));
                let p50 = sorted[sorted.len() / 2];
                let p95 = sorted[(sorted.len() * 95) / 100];
                (p50, p95)
            };

            // confidence: 1.0 if p95 < 500ms, degrades linearly to 0.0 at MAX_HOOK_OVERHEAD_MS
            let confidence = (1.0 - (p95 / MAX_HOOK_OVERHEAD_MS).min(1.0)) as f32;

            Ok(HandlerOutput::with_confidence(
                json!({"p50_ms": p50, "p95_ms": p95, "sample_count": durations.len()}),
                confidence,
            ))
        },
    );
}

fn register_detect_orphaned_worktrees(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new(handlers::TRIAGE_DETECT_ORPHANED_WORKTREES)
            .describe("Identify worktrees not on main or develop from git worktree list output.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let worktree_text = input
                .pointer("/worktree_list/output")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // git worktree list output: "<path>  <sha>  [<branch>]" or "(bare)"
            let orphans: Vec<Value> = worktree_text
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.contains("(bare)"))
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    let path = parts.first()?;
                    let branch = parts.get(2).map(|b| b.trim_matches(['[', ']']));
                    // Heuristic: worktrees not on main/develop are candidates
                    let is_main = branch
                        .map(|b| b == "main" || b == "develop")
                        .unwrap_or(false);
                    if !is_main {
                        Some(json!({"path": path, "branch": branch}))
                    } else {
                        None
                    }
                })
                .collect();

            Ok(json!({"orphaned_worktrees": orphans}))
        },
    );
}

fn register_build_cleanup_plan(registry: &mut HandlerRegistry) {
    registry.register_metadata(
        HandlerMetadata::new(handlers::TRIAGE_BUILD_CLEANUP_PLAN)
            .describe(
                "Build a cleanup confidence score from merged branch and orphaned worktree counts.",
            )
            .risk(RiskLevel::Low)
            .deterministic(true),
    );
    registry.handler(
        handlers::TRIAGE_BUILD_CLEANUP_PLAN,
        |input: Value| async move {
            let branches = input
                .get("branches")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let orphans = input
                .get("orphaned_worktrees")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);

            // confidence high = safe to auto-clean, low = needs manual review
            let confidence: f32 = if orphans > 0 {
                CONFIDENCE_ORPHANED_WORKTREES
            } else if branches > MANY_BRANCHES_THRESHOLD {
                CONFIDENCE_MANY_BRANCHES
            } else {
                CONFIDENCE_CLEAN_STATE
            };

            Ok(HandlerOutput::with_confidence(
                json!({"merged_branch_count": branches, "orphan_count": orphans}),
                confidence,
            ))
        },
    );
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
            .describe("Classify obfsck findings as true or false positives using file and context heuristics.")
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

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];

    for (i, row) in dp.iter_mut().enumerate().take(a.len() + 1) {
        row[0] = i;
    }
    for (j, val) in dp[0].iter_mut().enumerate().take(b.len() + 1) {
        *val = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[a.len()][b.len()]
}
