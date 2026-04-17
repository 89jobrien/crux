use crux_agentic::ctrl;
use cruxai_script::HandlerRegistry;
use serde_json::json;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    ctrl::register(&mut r);
    r
}

#[tokio::test]
async fn noop_passes_input_through() {
    let reg = registry();
    let handler = reg.get_handler("ctrl::noop").expect("handler missing");
    let input = json!({"data": 42});
    let result = handler(input.clone()).await.unwrap();
    assert_eq!(result, input);
}

#[tokio::test]
async fn log_passes_input_through() {
    let reg = registry();
    let handler = reg.get_handler("ctrl::log").expect("handler missing");
    let input = json!({"msg": "hello"});
    let result = handler(input.clone()).await.unwrap();
    assert_eq!(result, input);
}

#[tokio::test]
async fn assert_passes_when_condition_true() {
    let reg = registry();
    let handler = reg.get_handler("ctrl::assert").expect("handler missing");
    let input = json!({"args": {"condition": true, "message": "ok"}, "value": 1});
    let result = handler(input).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn assert_fails_when_condition_false() {
    let reg = registry();
    let handler = reg.get_handler("ctrl::assert").expect("handler missing");
    let input = json!({"args": {"condition": false, "message": "expected failure"}});
    let result = handler(input).await;
    assert!(result.is_err());
}
