use crux_script::HandlerRegistry;
use serde_json::json;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    crux_agentic::review::register(&mut r);
    r
}

#[tokio::test]
async fn normalize_findings_merges_sources() {
    let reg = registry();
    let h = reg.get_handler("review::normalize_findings").unwrap();
    let input = json!({
        "clippy": {"violations": [
            {"lint": "unused", "file": "a.rs", "line": 1, "message": "unused var"}
        ]},
        "arch": {"violations": [
            {"file": "b.rs", "imports": "infra::db", "violation": "domain imports infra"}
        ]},
        "coverage": {"uncovered": ["c.rs:10"]}
    });
    let out = h(input).await.unwrap();
    let findings = out.value["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 3);
    assert!(findings.iter().all(|f| f.get("source").is_some()));
}

#[tokio::test]
async fn apply_severity_tiers_findings() {
    let reg = registry();
    let h = reg.get_handler("review::apply_severity").unwrap();
    let input = json!({
        "findings": [
            {"source": "compile", "message": "error"},
            {"source": "clippy", "message": "suggestion"},
            {"source": "coverage", "message": "uncovered"},
        ]
    });
    let out = h(input).await.unwrap();
    let findings = out.value["findings"].as_array().unwrap();
    assert_eq!(findings[0]["tier"], "blocking");
    assert_eq!(findings[1]["tier"], "suggestion");
    assert_eq!(findings[2]["tier"], "observation");
}

#[tokio::test]
async fn compute_score_emits_confidence() {
    let reg = registry();
    let h = reg.get_handler("review::compute_score").unwrap();
    let input = json!({
        "findings": [
            {"tier": "blocking", "file": "a.rs"},
            {"tier": "suggestion", "file": "b.rs"},
            {"tier": "observation", "file": "c.rs"},
        ]
    });
    let out = h(input).await.unwrap();
    assert!(out.confidence.is_some());
    let score = out.confidence.unwrap();
    assert!(score < 1.0);
}

#[tokio::test]
async fn compute_score_perfect_when_no_blocking() {
    let reg = registry();
    let h = reg.get_handler("review::compute_score").unwrap();
    let input = json!({
        "findings": [
            {"tier": "suggestion"},
            {"tier": "observation"},
        ]
    });
    let out = h(input).await.unwrap();
    assert_eq!(out.confidence.unwrap(), 1.0);
}

#[tokio::test]
async fn compute_score_empty_findings() {
    let reg = registry();
    let h = reg.get_handler("review::compute_score").unwrap();
    let out = h(json!({"findings": []})).await.unwrap();
    assert_eq!(out.confidence.unwrap(), 1.0);
}
