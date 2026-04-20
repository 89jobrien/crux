use cruxx_agentic::json as json_handlers;
use cruxx_script::HandlerRegistry;
use serde_json::json;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    json_handlers::register(&mut r);
    r
}

#[tokio::test]
async fn pick_extracts_fields() {
    let reg = registry();
    let handler = reg.get_handler("json::pick").unwrap();
    let input = json!({
        "args": {"fields": ["a", "c"]},
        "a": 1, "b": 2, "c": 3
    });
    let result = handler(input).await.unwrap();
    assert_eq!(result["a"], 1);
    assert_eq!(result["c"], 3);
    assert!(result.get("b").is_none());
    assert!(result.get("args").is_none());
}

#[tokio::test]
async fn merge_combines_objects() {
    let reg = registry();
    let handler = reg.get_handler("json::merge").unwrap();
    let input = json!({
        "args": {"with": {"b": 2, "c": 3}},
        "a": 1, "b": 0
    });
    let result = handler(input).await.unwrap();
    assert_eq!(result["a"], 1);
    assert_eq!(result["b"], 2);
    assert_eq!(result["c"], 3);
    assert!(result.get("args").is_none());
}

#[tokio::test]
async fn jq_simple_field_access() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input = json!({"args": {"expr": ".name"}, "name": "alice"});
    let result = handler(input).await.unwrap();
    assert_eq!(result, json!("alice"));
}

#[tokio::test]
async fn jq_nested_field_access() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input = json!({"args": {"expr": ".user.age"}, "user": {"age": 30}});
    let result = handler(input).await.unwrap();
    assert_eq!(result, json!(30));
}

#[tokio::test]
async fn jq_array_index() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input = json!({"args": {"expr": ".items.[1]"}, "items": ["a", "b", "c"]});
    let result = handler(input).await.unwrap();
    assert_eq!(result, json!("b"));
}

#[tokio::test]
async fn jq_missing_path_returns_null() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input = json!({"args": {"expr": ".missing"}, "other": 1});
    let result = handler(input).await.unwrap();
    assert_eq!(result, json!(null));
}
