use cruxx_script::HandlerRegistry;
use serde_json::json;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    cruxx_agentic::ci::register(&mut r);
    r
}

#[tokio::test]
async fn compile_errors_parses_rustc_output() {
    let reg = registry();
    let h = reg.get_handler("ci::compile_errors").unwrap();
    let input = json!({
        "log": "error[E0308]: mismatched types\n --> src/main.rs:10:5\n"
    });
    let out = h(input).await.unwrap();
    let errors = out.value["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["code"], "E0308");
    assert_eq!(errors[0]["file"], "src/main.rs");
    assert_eq!(errors[0]["line"], 10);
}

#[tokio::test]
async fn compile_errors_empty_log() {
    let reg = registry();
    let h = reg.get_handler("ci::compile_errors").unwrap();
    let out = h(json!({"log": ""})).await.unwrap();
    assert_eq!(out.value["errors"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn clippy_violations_parses_warnings() {
    let reg = registry();
    let h = reg.get_handler("ci::clippy_violations").unwrap();
    let input = json!({
        "log": "warning: unused variable: `x`\n --> src/lib.rs:5:9\n = note: `#[warn(unused_variables)]`\n"
    });
    let out = h(input).await.unwrap();
    let violations = out.value["violations"].as_array().unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0]["file"], "src/lib.rs");
}

#[tokio::test]
async fn nextest_failures_parses_test_names() {
    let reg = registry();
    let h = reg.get_handler("ci::nextest_failures").unwrap();
    let input = json!({
        "log": "     FAIL [   0.123s] my-crate::tests::test_foo\n--- STDOUT: ---\nthread 'tests::test_foo' panicked at 'assert failed'\n"
    });
    let out = h(input).await.unwrap();
    let failures = out.value["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 1);
    assert!(
        failures[0]["test_name"]
            .as_str()
            .unwrap()
            .contains("test_foo")
    );
}

#[tokio::test]
async fn deny_violations_parses_cargo_deny() {
    let reg = registry();
    let h = reg.get_handler("ci::deny_violations").unwrap();
    let input = json!({
        "log": "error[banned]: crate openssl is banned\nerror[license]: crate foo has unapproved license GPL-3.0\n"
    });
    let out = h(input).await.unwrap();
    let violations = out.value["violations"].as_array().unwrap();
    assert_eq!(violations.len(), 2);
}

#[tokio::test]
async fn deduplicate_spans_merges_same_source_and_location() {
    let reg = registry();
    let h = reg.get_handler("ci::deduplicate_spans").unwrap();
    let input = json!({
        "errors": [
            {"source": "compile", "file": "src/a.rs", "line": 10, "message": "err1"},
            {"source": "compile", "file": "src/a.rs", "line": 10, "message": "err2"},
            {"source": "clippy", "file": "src/a.rs", "line": 10, "message": "lint"},
            {"source": "compile", "file": "src/b.rs", "line": 5, "message": "err3"},
        ]
    });
    let out = h(input).await.unwrap();
    let deduped = out.value["errors"].as_array().unwrap();
    // compile:src/a.rs:10, clippy:src/a.rs:10, compile:src/b.rs:5
    assert_eq!(deduped.len(), 3);
}

#[tokio::test]
async fn classify_severity_orders_correctly() {
    let reg = registry();
    let h = reg.get_handler("ci::classify_severity").unwrap();
    let input = json!({
        "items": [
            {"source": "clippy", "message": "lint"},
            {"source": "compile", "message": "error"},
            {"source": "test", "message": "fail"},
            {"source": "deny", "message": "banned"},
        ]
    });
    let out = h(input).await.unwrap();
    let ranked = out.value["ranked"].as_array().unwrap();
    assert_eq!(ranked[0]["source"], "compile");
    assert_eq!(ranked[1]["source"], "deny");
}

#[tokio::test]
async fn score_fixability_emits_confidence() {
    let reg = registry();
    let h = reg.get_handler("ci::score_fixability").unwrap();
    let input = json!({
        "ranked": [
            {"source": "clippy", "message": "unused import"},
            {"source": "compile", "message": "missing lifetime"},
        ]
    });
    let out = h(input).await.unwrap();
    assert!(out.confidence.is_some());
    // 1 clippy out of 2 = 0.5
    assert_eq!(out.confidence.unwrap(), 0.5);
}
