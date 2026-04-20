use cruxx::prelude::*;

#[cruxx::evolve]
async fn optimize_container(profile: HarnessProfile) -> Crux<HarnessProfile> {
    let mut candidate = profile.clone();
    candidate.resources.memory_mb += 256;
    candidate.id = format!("{}-evolved", profile.id);
    Ok(candidate)
}

#[test]
fn evolve_macro_produces_agent_struct() {
    let name = OptimizeContainerAgent::name();
    assert_eq!(name, "optimize_container");
}

#[test]
fn evolve_macro_is_marked_as_evolution_agent() {
    assert!(OptimizeContainerAgent::is_evolution_agent());
}

#[tokio::test]
async fn evolve_macro_runs_function() {
    let profile = HarnessProfile {
        id: "test-v1".into(),
        resources: ResourceHints {
            memory_mb: 512,
            cpu_millicores: 1000,
            timeout_seconds: 300,
        },
        network_access: false,
        allowed_syscalls: vec![],
    };
    let result = optimize_container(profile).await;
    let value = result.value();
    assert!(value.is_ok());
    let output = value.as_ref().unwrap();
    assert_eq!(output.resources.memory_mb, 768);
}
