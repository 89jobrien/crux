//! Integration tests for trace-analysis and optimization handlers.

use crux_script::HandlerRegistry;
use serde_json::json;

// ---------------------------------------------------------------------------
// Trace JSON helpers for replay_dry_run tests
// ---------------------------------------------------------------------------

/// Build a minimal valid trace JSON string with the given steps.
///
/// Each step entry is expected to have at least `name`, `status`, `kind`,
/// `confidence`, `started_at`, `duration_ms`, `input_hash`, and `attempt`.
fn minimal_trace_json(agent: &str, steps: &[serde_json::Value]) -> String {
    serde_json::to_string(&json!({
        "id": "crux_01J0000000000000000000000",
        "agent": agent,
        "value": {"Ok": null},
        "steps": steps,
        "children": [],
        "started_at": "2026-01-01T00:00:00Z",
        "finished_at": "2026-01-01T00:00:01Z"
    }))
    .unwrap()
}

fn ok_step(name: &str) -> serde_json::Value {
    json!({
        "name": name,
        "kind": "Plain",
        "status": "ok",
        "origin": "live",
        "confidence": 1.0,
        "started_at": "2026-01-01T00:00:00Z",
        "duration_ms": 10,
        "input_hash": 42,
        "attempt": 1
    })
}

fn err_step(name: &str) -> serde_json::Value {
    json!({
        "name": name,
        "kind": "Plain",
        "status": "err",
        "origin": "live",
        "confidence": 0.0,
        "started_at": "2026-01-01T00:00:00Z",
        "duration_ms": 5,
        "input_hash": 99,
        "attempt": 1,
        "error": "step failed"
    })
}

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    crux_agentic::analysis::register(&mut r);
    r
}

#[tokio::test]
async fn latency_profile_flags_slow_steps() {
    let reg = registry();
    let h = reg.get_handler("analysis::latency_profile").unwrap();
    let input = json!({
        "steps": [
            {"name": "a", "started_at": "2026-01-01T00:00:00Z",
             "completed_at": "2026-01-01T00:00:01Z"},
            {"name": "b", "started_at": "2026-01-01T00:00:00Z",
             "completed_at": "2026-01-01T00:00:10Z"},
            {"name": "c", "started_at": "2026-01-01T00:00:00Z",
             "completed_at": "2026-01-01T00:00:01Z"},
        ]
    });
    let out = h(input).await.outcome.unwrap();
    let slow = out.value["slow_steps"].as_array().unwrap();
    assert_eq!(slow.len(), 1);
    assert_eq!(slow[0]["name"], "b");
}

#[tokio::test]
async fn latency_profile_empty_steps() {
    let reg = registry();
    let h = reg.get_handler("analysis::latency_profile").unwrap();
    let out = h(json!({"steps": []})).await.outcome.unwrap();
    assert_eq!(out.value["slow_steps"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn token_spend_top3() {
    let reg = registry();
    let h = reg.get_handler("analysis::token_spend").unwrap();
    let input = json!({
        "steps": [
            {"name": "a", "output": {"metadata": {"tokens": 100}}},
            {"name": "b", "output": {"metadata": {"tokens": 500}}},
            {"name": "c", "output": {"metadata": {"tokens": 200}}},
            {"name": "d", "output": {"metadata": {"tokens": 50}}},
        ]
    });
    let out = h(input).await.outcome.unwrap();
    assert_eq!(out.value["total"], 850);
    let top3 = out.value["top3"].as_array().unwrap();
    assert_eq!(top3[0], "b");
}

#[tokio::test]
async fn failure_clusters_groups_by_kind() {
    let reg = registry();
    let h = reg.get_handler("analysis::failure_clusters").unwrap();
    let input = json!({
        "steps": [
            {"name": "x", "status": "failed", "error": {"kind": "StepFailed"}},
            {"name": "y", "status": "failed", "error": {"kind": "StepFailed"}},
            {"name": "z", "status": "failed", "error": {"kind": "Timeout"}},
        ]
    });
    let out = h(input).await.outcome.unwrap();
    let clusters = out.value["clusters"].as_array().unwrap();
    assert_eq!(clusters.len(), 2);
}

#[tokio::test]
async fn replay_cache_hit_ratio() {
    let reg = registry();
    let h = reg.get_handler("analysis::replay_cache_hits").unwrap();
    let input = json!({
        "steps": [
            {"name": "a", "cache_hit": true},
            {"name": "a", "cache_hit": false},
            {"name": "b", "cache_hit": true},
            {"name": "b", "cache_hit": true},
        ]
    });
    let out = h(input).await.outcome.unwrap();
    let by_step = out.value["by_step"].as_array().unwrap();
    let a = by_step.iter().find(|s| s["name"] == "a").unwrap();
    assert_eq!(a["hits"], 1);
    assert_eq!(a["misses"], 1);
}

#[tokio::test]
async fn tighten_budget_emits_suggestion_above_threshold() {
    let reg = registry();
    let h = reg.get_handler("analysis::tighten_budget").unwrap();
    let input = json!({
        "args": {},
        "token_spend": {"total": 900},
        "budget": {"tokens": 1000}
    });
    let out = h(input).await.outcome.unwrap();
    assert!(out.confidence.is_some());
    assert!(out.value.get("suggestion").is_some());
}

#[tokio::test]
async fn tighten_budget_skips_when_under_threshold() {
    let reg = registry();
    let h = reg.get_handler("analysis::tighten_budget").unwrap();
    let input = json!({
        "args": {},
        "token_spend": {"total": 500},
        "budget": {"tokens": 1000}
    });
    let out = h(input).await.outcome.unwrap();
    assert!(out.value.get("suggestion").is_none());
}

#[tokio::test]
async fn tune_retry_suggests_backoff_for_flaky() {
    let reg = registry();
    let h = reg.get_handler("analysis::tune_retry").unwrap();
    let input = json!({
        "failure_clusters": {
            "clusters": [
                {"kind": "StepFailed", "count": 3, "step_names": ["flaky_step"]},
                {"kind": "Timeout", "count": 1, "step_names": ["ok_step"]},
            ]
        }
    });
    let out = h(input).await.outcome.unwrap();
    let suggestions = out.value["suggestions"].as_array().unwrap();
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0]["step_name"], "flaky_step");
}

#[tokio::test]
async fn compress_stages_flags_heavy() {
    let reg = registry();
    let h = reg.get_handler("analysis::compress_stages").unwrap();
    let input = json!({
        "token_spend": {
            "by_step": [
                {"name": "heavy", "tokens": 500},
                {"name": "light", "tokens": 100},
            ],
            "total": 600
        }
    });
    let out = h(input).await.outcome.unwrap();
    let suggestions = out.value["suggestions"].as_array().unwrap();
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0]["stage"], "heavy");
}

#[tokio::test]
async fn patch_schema_check_valid_yaml() {
    let reg = registry();
    let h = reg.get_handler("analysis::patch_schema_check").unwrap();
    let input = json!({"patch": "key: value\nlist:\n  - item1\n"});
    let out = h(input).await.outcome.unwrap();
    assert_eq!(out.value["valid"], true);
}

#[tokio::test]
async fn patch_schema_check_invalid_yaml() {
    let reg = registry();
    let h = reg.get_handler("analysis::patch_schema_check").unwrap();
    let input = json!({"patch": "key: [invalid\n"});
    let out = h(input).await.outcome.unwrap();
    assert_eq!(out.value["valid"], false);
}

#[tokio::test]
async fn patch_schema_check_empty() {
    let reg = registry();
    let h = reg.get_handler("analysis::patch_schema_check").unwrap();
    let input = json!({"patch": ""});
    let out = h(input).await.outcome.unwrap();
    assert_eq!(out.value["valid"], false);
}

// ---------------------------------------------------------------------------
// replay_dry_run — in-process analysis
// ---------------------------------------------------------------------------

#[tokio::test]
async fn replay_dry_run_missing_trace_path() {
    let reg = registry();
    let h = reg.get_handler("analysis::replay_dry_run").unwrap();
    let out = h(json!({})).await.outcome.unwrap();
    assert_eq!(out.value["ok"], false);
    let mismatches = out.value["mismatches"].as_array().unwrap();
    assert!(
        mismatches
            .iter()
            .any(|m| m.as_str().unwrap().contains("missing trace_path")),
        "expected missing trace_path in mismatches: {mismatches:?}"
    );
}

#[tokio::test]
async fn replay_dry_run_invalid_path_chars() {
    let reg = registry();
    let h = reg.get_handler("analysis::replay_dry_run").unwrap();
    // Leading dash triggers the injection guard.
    let out = h(json!({"trace_path": "-rm rf /"})).await;
    assert!(
        out.outcome.is_err(),
        "expected error for suspicious path, got ok"
    );
}

#[tokio::test]
async fn replay_dry_run_clean_trace_ok() {
    let reg = registry();
    let h = reg.get_handler("analysis::replay_dry_run").unwrap();

    let trace = minimal_trace_json("test-agent", &[ok_step("fetch"), ok_step("parse")]);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &trace).unwrap();

    let out = h(json!({"trace_path": tmp.path().to_str().unwrap()}))
        .await
        .outcome
        .unwrap();
    assert_eq!(out.value["ok"], true);
    assert_eq!(out.value["step_count"], 2);
    assert_eq!(out.value["agent"], "test-agent");
    assert_eq!(
        out.value["mismatches"].as_array().unwrap().len(),
        0,
        "expected no mismatches"
    );
}

#[tokio::test]
async fn replay_dry_run_detects_failed_step() {
    let reg = registry();
    let h = reg.get_handler("analysis::replay_dry_run").unwrap();

    let trace = minimal_trace_json("test-agent", &[ok_step("fetch"), err_step("validate")]);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &trace).unwrap();

    let out = h(json!({"trace_path": tmp.path().to_str().unwrap()}))
        .await
        .outcome
        .unwrap();
    assert_eq!(out.value["ok"], false);
    let mismatches = out.value["mismatches"].as_array().unwrap();
    assert!(
        mismatches
            .iter()
            .any(|m| m.as_str().unwrap().contains("validate")),
        "expected 'validate' step in mismatches: {mismatches:?}"
    );
}

#[tokio::test]
async fn replay_dry_run_patch_ordinal_mismatch() {
    let reg = registry();
    let h = reg.get_handler("analysis::replay_dry_run").unwrap();

    // Trace has: fetch, parse
    let trace = minimal_trace_json("test-agent", &[ok_step("fetch"), ok_step("parse")]);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &trace).unwrap();

    // Patch says steps are: fetch, transform  — "transform" != "parse" at ordinal 1
    let patch = "steps:\n  - name: fetch\n  - name: transform\n";

    let out = h(json!({
        "trace_path": tmp.path().to_str().unwrap(),
        "patch": patch,
    }))
    .await
    .outcome
    .unwrap();
    assert_eq!(out.value["ok"], false);
    let mismatches = out.value["mismatches"].as_array().unwrap();
    assert!(
        mismatches
            .iter()
            .any(|m| m.as_str().unwrap().contains("transform")),
        "expected ordinal mismatch for 'transform': {mismatches:?}"
    );
}

#[tokio::test]
async fn replay_dry_run_patch_aligns_returns_ok() {
    let reg = registry();
    let h = reg.get_handler("analysis::replay_dry_run").unwrap();

    // Trace has: fetch, parse
    let trace = minimal_trace_json("test-agent", &[ok_step("fetch"), ok_step("parse")]);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &trace).unwrap();

    // Patch step names match trace exactly
    let patch = "steps:\n  - name: fetch\n  - name: parse\n";

    let out = h(json!({
        "trace_path": tmp.path().to_str().unwrap(),
        "patch": patch,
    }))
    .await
    .outcome
    .unwrap();
    assert_eq!(out.value["ok"], true);
}

#[tokio::test]
async fn replay_dry_run_malformed_trace_returns_err() {
    let reg = registry();
    let h = reg.get_handler("analysis::replay_dry_run").unwrap();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), b"not valid json {{{").unwrap();

    let out = h(json!({"trace_path": tmp.path().to_str().unwrap()})).await;
    assert!(
        out.outcome.is_err(),
        "expected error for malformed trace JSON"
    );
}

#[tokio::test]
async fn replay_dry_run_nonexistent_file_returns_err() {
    let reg = registry();
    let h = reg.get_handler("analysis::replay_dry_run").unwrap();

    let out = h(json!({"trace_path": "/tmp/does-not-exist-crux-test-12345.json"})).await;
    assert!(
        out.outcome.is_err(),
        "expected error for missing trace file"
    );
}
