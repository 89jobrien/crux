use cruxx_script::HandlerRegistry;
use serde_json::json;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    cruxx_agentic::triage::register(&mut r);
    r
}

#[tokio::test]
async fn parse_repo_tags_extracts_repo() {
    let reg = registry();
    let h = reg.get_handler("triage::parse_repo_tags").unwrap();
    let input = json!({
        "todos": [
            {"id": "1", "title": "fix bug", "metadata": {"repo": "crux"}},
            {"id": "2", "title": "add test", "metadata": {"repo": "minibox"}},
        ]
    });
    let out = h(input).await.unwrap();
    let todos = out.value["todos"].as_array().unwrap();
    assert_eq!(todos[0]["repo"], "crux");
    assert_eq!(todos[1]["repo"], "minibox");
}

#[tokio::test]
async fn score_urgency_sorts_by_score() {
    let reg = registry();
    let h = reg.get_handler("triage::score_urgency").unwrap();
    let input = json!({
        "todos": [
            {"id": "1", "priority": "low", "created_at": "2026-01-01T00:00:00Z"},
            {"id": "2", "priority": "high", "created_at": "2026-05-01T00:00:00Z"},
            {"id": "3", "priority": "high", "created_at": "2026-01-01T00:00:00Z"},
        ]
    });
    let out = h(input).await.unwrap();
    let todos = out.value["todos"].as_array().unwrap();
    // Oldest high-priority first
    assert_eq!(todos[0]["id"], "3");
}

#[tokio::test]
async fn deduplicate_intent_clusters_similar() {
    let reg = registry();
    let h = reg.get_handler("triage::deduplicate_intent").unwrap();
    let input = json!({
        "todos": [
            {"id": "1", "title": "fix login bug"},
            {"id": "2", "title": "fix login bug in auth"},
            {"id": "3", "title": "add dark mode"},
        ]
    });
    let out = h(input).await.unwrap();
    let groups = out.value["groups"].as_array().unwrap();
    assert!(groups.len() <= 2);
}

#[tokio::test]
async fn group_by_repo_partitions() {
    let reg = registry();
    let h = reg.get_handler("triage::group_by_repo").unwrap();
    let input = json!({
        "todos": [
            {"id": "1", "repo": "crux"},
            {"id": "2", "repo": "minibox"},
            {"id": "3", "repo": "crux"},
        ]
    });
    let out = h(input).await.unwrap();
    let repos = out.value["repos"].as_object().unwrap();
    assert_eq!(repos["crux"].as_array().unwrap().len(), 2);
    assert_eq!(repos["minibox"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn group_by_repo_empty() {
    let reg = registry();
    let h = reg.get_handler("triage::group_by_repo").unwrap();
    let out = h(json!({"todos": []})).await.unwrap();
    assert_eq!(out.value["repos"].as_object().unwrap().len(), 0);
}
