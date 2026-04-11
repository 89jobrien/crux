/// End-to-end tests for #[crux::agent] proc macro.
///
/// These tests verify that the macro generates correct Agent impls,
/// wrapper functions, and integrates with CruxCtx lifecycle hooks.
use crux::prelude::*;

// -- Basic: zero-param agent ------------------------------------------------

#[crux::agent]
async fn greet() -> Crux<String> {
    Ok("hello".to_string())
}

#[tokio::test]
async fn zero_param_agent() {
    let crux = greet().await;
    assert_eq!(crux.value().unwrap(), "hello");
    assert_eq!(crux.agent, "greet");
    assert!(crux.finished_at.is_some());
}

#[test]
fn zero_param_agent_trait_name() {
    assert_eq!(GreetAgent::name(), "greet");
}

// -- Basic: single-param agent ----------------------------------------------

#[crux::agent]
async fn echo(msg: String) -> Crux<String> {
    Ok(msg)
}

#[tokio::test]
async fn single_param_agent() {
    let crux = echo("hi".to_string()).await;
    assert_eq!(crux.value().unwrap(), "hi");
    assert_eq!(crux.agent, "echo");
}

#[test]
fn single_param_agent_trait_name() {
    assert_eq!(EchoAgent::name(), "echo");
}

// -- Basic: multi-param agent -----------------------------------------------

#[crux::agent]
async fn add(a: i32, b: i32) -> Crux<i32> {
    Ok(a + b)
}

#[tokio::test]
async fn multi_param_agent() {
    let crux = add(3, 4).await;
    assert_eq!(crux.value().unwrap(), &7);
    assert_eq!(crux.agent, "add");
}

#[test]
fn multi_param_agent_trait_name() {
    assert_eq!(AddAgent::name(), "add");
}

// -- Agent with steps -------------------------------------------------------

#[crux::agent]
async fn two_step(input: String) -> Crux<String> {
    let upper: String = t.step("uppercase", || {
        let inp = input.clone();
        async move { Ok(inp.to_uppercase()) }
    }).await?;

    let result: String = t.step("append", || {
        let u = upper.clone();
        async move { Ok(format!("{u}!")) }
    }).await?;

    Ok(result)
}

#[tokio::test]
async fn agent_with_steps_records_trace() {
    let crux = two_step("hello".to_string()).await;
    assert_eq!(crux.value().unwrap(), "HELLO!");
    assert_eq!(crux.steps.len(), 2);
    assert_eq!(crux.steps[0].name, "uppercase");
    assert_eq!(crux.steps[1].name, "append");
    assert!(crux.steps.iter().all(|s| s.is_ok()));
}

// -- Agent that fails -------------------------------------------------------

#[crux::agent]
async fn fallible(should_fail: bool) -> Crux<String> {
    if should_fail {
        return Err(CruxErr::step_failed("check", "intentional failure"));
    }
    Ok("success".to_string())
}

#[tokio::test]
async fn agent_failure_captured_in_crux() {
    let crux = fallible(true).await;
    assert!(crux.value().is_err());

    let crux_ok = fallible(false).await;
    assert_eq!(crux_ok.value().unwrap(), "success");
}

// -- Agent uses t.step that fails -------------------------------------------

#[crux::agent]
async fn step_fails() -> Crux<i32> {
    let _: i32 = t.step("ok_step", || async { Ok(1) }).await?;
    let _: i32 = t.step("bad_step", || async {
        Err(CruxErr::step_failed("bad_step", "oops"))
    }).await?;
    Ok(0) // unreachable
}

#[tokio::test]
async fn step_failure_propagates_through_macro() {
    let crux = step_fails().await;
    assert!(crux.value().is_err());
    // First step succeeded, second failed
    assert_eq!(crux.steps.len(), 2);
    assert!(crux.steps[0].is_ok());
    assert!(crux.steps[1].is_err());
}

// -- Agent with confidence --------------------------------------------------

#[crux::agent]
async fn uncertain() -> Crux<String> {
    let val: String = t.step_with_confidence("guess", 0.4, || async {
        Ok("maybe".to_string())
    }).await?;
    Ok(val)
}

#[tokio::test]
async fn step_confidence_recorded() {
    let crux = uncertain().await;
    assert_eq!(crux.value().unwrap(), "maybe");
    assert_eq!(crux.steps[0].confidence, 0.4);
}

// -- Agent with lifecycle hooks via t ---------------------------------------

#[crux::agent]
async fn hooked() -> Crux<i32> {
    t.on_step_failure(|_err| async {
        Recovery::Substitute(serde_json::json!(42))
    });

    let val: i32 = t.step("will_fail", || async {
        Err(CruxErr::step_failed("will_fail", "expected"))
    }).await?;

    Ok(val)
}

#[tokio::test]
async fn lifecycle_hooks_work_through_macro() {
    let crux = hooked().await;
    assert_eq!(crux.value().unwrap(), &42);
}

// -- Agent struct naming: snake_case -> PascalCase --------------------------

#[crux::agent]
async fn my_complex_name() -> Crux<bool> {
    Ok(true)
}

#[test]
fn snake_to_pascal_naming() {
    assert_eq!(MyComplexNameAgent::name(), "my_complex_name");
}

// -- Agent::run can be called directly with CruxCtx -------------------------

#[tokio::test]
async fn agent_run_directly() {
    let mut ctx = CruxCtx::new("direct_test");
    let result = <EchoAgent as Agent>::run(&mut ctx, "direct".to_string()).await;
    assert_eq!(result.unwrap(), "direct");
}

// -- Wrapper function returns Crux with correct metadata --------------------

#[tokio::test]
async fn wrapper_produces_complete_crux() {
    let crux = greet().await;

    // Has an ID
    assert!(!crux.id.as_str().is_empty());
    assert!(crux.id.as_str().starts_with("crux_"));

    // Has timing
    assert!(crux.started_at <= crux.finished_at.unwrap());

    // No children (no delegation)
    assert!(crux.children.is_empty());
}

// -- Serialization of macro-generated Crux ----------------------------------

#[tokio::test]
async fn crux_from_macro_serializes() {
    let crux = two_step("test".to_string()).await;
    let json = serde_json::to_string_pretty(&crux).unwrap();
    let back: Crux<String> = serde_json::from_str(&json).unwrap();
    assert_eq!(back.value().unwrap(), "TEST!");
    assert_eq!(back.steps.len(), 2);
}
