use cruxx_script::HandlerRegistry;
use serde_json::json;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    cruxx_agentic::analysis::register(&mut r);
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
    let out = h(input).await.unwrap();
    let slow = out.value["slow_steps"].as_array().unwrap();
    assert_eq!(slow.len(), 1);
    assert_eq!(slow[0]["name"], "b");
}

#[tokio::test]
async fn latency_profile_empty_steps() {
    let reg = registry();
    let h = reg.get_handler("analysis::latency_profile").unwrap();
    let out = h(json!({"steps": []})).await.unwrap();
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
    let out = h(input).await.unwrap();
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
    let out = h(input).await.unwrap();
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
    let out = h(input).await.unwrap();
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
    let out = h(input).await.unwrap();
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
    let out = h(input).await.unwrap();
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
    let out = h(input).await.unwrap();
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
    let out = h(input).await.unwrap();
    let suggestions = out.value["suggestions"].as_array().unwrap();
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0]["stage"], "heavy");
}

#[tokio::test]
async fn patch_schema_check_valid_yaml() {
    let reg = registry();
    let h = reg.get_handler("analysis::patch_schema_check").unwrap();
    let input = json!({"patch": "key: value\nlist:\n  - item1\n"});
    let out = h(input).await.unwrap();
    assert_eq!(out.value["valid"], true);
}

#[tokio::test]
async fn patch_schema_check_invalid_yaml() {
    let reg = registry();
    let h = reg.get_handler("analysis::patch_schema_check").unwrap();
    let input = json!({"patch": "key: [invalid\n"});
    let out = h(input).await.unwrap();
    assert_eq!(out.value["valid"], false);
}

#[tokio::test]
async fn patch_schema_check_empty() {
    let reg = registry();
    let h = reg.get_handler("analysis::patch_schema_check").unwrap();
    let input = json!({"patch": ""});
    let out = h(input).await.unwrap();
    assert_eq!(out.value["valid"], false);
}
