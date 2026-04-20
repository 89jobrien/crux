use cruxx_agentic::harness;
use cruxx_script::HandlerRegistry;
use serde_json::json;

fn registry() -> HandlerRegistry {
    let mut r = HandlerRegistry::new();
    harness::register(&mut r);
    r
}

#[tokio::test]
async fn harness_handlers_registered() {
    let reg = registry();
    assert!(reg.get_handler("harness::evolve").is_some());
    assert!(reg.get_handler("harness::canary").is_some());
}

#[tokio::test]
async fn harness_canary_returns_outcome() {
    let reg = registry();
    let handler = reg.get_handler("harness::canary").unwrap();
    let input = json!({
        "args": {
            "baseline_profile": {
                "id": "default-v1",
                "resources": {"memory_mb": 512, "cpu_millicores": 1000, "timeout_seconds": 300},
                "network_access": false,
                "allowed_syscalls": ["read", "write"]
            },
            "candidate_profile": {
                "id": "evolved-v2",
                "resources": {"memory_mb": 768, "cpu_millicores": 1000, "timeout_seconds": 300},
                "network_access": false,
                "allowed_syscalls": ["read", "write"]
            },
            "eval_image": "test-suite:latest",
            "eval_cmd": ["./run-benchmarks"]
        }
    });
    let result = handler(input).await.unwrap().value;
    assert!(result.get("outcome").is_some());
}
