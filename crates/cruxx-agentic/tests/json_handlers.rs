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

#[tokio::test]
async fn jq_keys_returns_sorted_keys() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input = json!({"args": {"expr": "keys"}, "b": 2, "a": 1});
    let result = handler(input).await.unwrap();
    // keys of the payload (args key excluded)
    assert!(result.is_array());
    let arr: Vec<&str> = result
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(arr.contains(&"a"));
    assert!(arr.contains(&"b"));
    assert!(!arr.contains(&"args"));
}

#[tokio::test]
async fn jq_length_returns_count() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input = json!({"args": {"expr": ".items | length"}, "items": [1, 2, 3]});
    let result = handler(input).await.unwrap();
    assert_eq!(result, json!(3));
}

#[tokio::test]
async fn jq_type_returns_type_string() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input = json!({"args": {"expr": ".count | type"}, "count": 42});
    let result = handler(input).await.unwrap();
    assert_eq!(result, json!("number"));
}

#[tokio::test]
async fn jq_has_key_returns_bool() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input_yes = json!({"args": {"expr": "has(\"name\")"}, "name": "alice"});
    let result = handler(input_yes).await.unwrap();
    assert_eq!(result, json!(true));
}

#[tokio::test]
async fn jq_first_returns_first_element() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input = json!({"args": {"expr": ".items | first"}, "items": ["x", "y"]});
    let result = handler(input).await.unwrap();
    assert_eq!(result, json!("x"));
}

#[tokio::test]
async fn jq_last_returns_last_element() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input = json!({"args": {"expr": ".items | last"}, "items": ["x", "y", "z"]});
    let result = handler(input).await.unwrap();
    assert_eq!(result, json!("z"));
}

#[tokio::test]
async fn jq_dot_path_still_works() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input = json!({"args": {"expr": ".foo.bar"}, "foo": {"bar": 42}});
    let result = handler(input).await.unwrap();
    assert_eq!(result, json!(42));
}

#[tokio::test]
async fn jq_unsupported_syntax_returns_error() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input = json!({"args": {"expr": "select(.x > 1)"}, "x": 2});
    let err = handler(input).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("json::jq only supports"),
        "expected unsupported-syntax error, got: {msg}"
    );
    assert!(
        msg.contains("shell::capture"),
        "expected shell::capture hint, got: {msg}"
    );
}

#[tokio::test]
async fn jq_map_syntax_returns_error() {
    let reg = registry();
    let handler = reg.get_handler("json::jq").unwrap();
    let input = json!({"args": {"expr": "map(.x)"}, "items": [{"x": 1}]});
    let err = handler(input).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("json::jq only supports"),
        "expected unsupported-syntax error, got: {msg}"
    );
}
