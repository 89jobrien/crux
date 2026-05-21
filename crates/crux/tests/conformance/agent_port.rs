/// Conformance tests: Agent port — macro-generated vs hand-written impl equivalence.
///
/// Verifies both paths satisfy the Agent trait contract:
/// - name() matches the function/struct name
/// - run() produces a Crux<T> with the correct agent field and value
/// - Steps recorded inside run() appear in the returned trace
/// - Default lifecycle hooks return the specified sentinel values
use crux::prelude::*;

// ── macro-generated agent ────────────────────────────────────────────────────

#[crux::agent]
async fn macro_add(a: u32, b: u32) -> Crux<u32> {
    let sum = x
        .step("add", || async { Ok::<u32, CruxErr>(a + b) })
        .await?;
    Ok(sum)
}

#[tokio::test]
async fn conformance_agent_macro_name_matches_fn_name() {
    assert_eq!(MacroAddAgent::name(), "macro_add");
}

#[tokio::test]
async fn conformance_agent_macro_run_returns_correct_value() {
    let crux = macro_add(3, 4).await;
    assert_eq!(crux.value().unwrap(), &7);
}

#[tokio::test]
async fn conformance_agent_macro_crux_agent_field_is_fn_name() {
    let crux = macro_add(1, 2).await;
    assert_eq!(crux.agent, "macro_add");
}

#[tokio::test]
async fn conformance_agent_macro_steps_recorded_in_trace() {
    let crux = macro_add(10, 20).await;
    assert!(
        crux.steps.iter().any(|s| s.name == "add"),
        "step 'add' must appear in trace"
    );
}

#[tokio::test]
async fn conformance_agent_macro_finished_at_is_set() {
    let crux = macro_add(0, 0).await;
    assert!(
        crux.finished_at.is_some(),
        "finished_at must be set after successful run"
    );
}

// ── hand-written Agent impl ──────────────────────────────────────────────────

struct DoubleAgent;

impl Agent for DoubleAgent {
    type Input = u32;
    type Output = u32;

    fn name() -> &'static str {
        "double"
    }

    async fn run(ctx: &mut crux::ctx::CruxCtx, input: u32) -> Result<u32, CruxErr> {
        ctx.step(
            "double_inner",
            || async move { Ok::<u32, CruxErr>(input * 2) },
        )
        .await
    }
}

#[tokio::test]
async fn conformance_agent_handwritten_name() {
    assert_eq!(DoubleAgent::name(), "double");
}

#[tokio::test]
async fn conformance_agent_handwritten_default_budget_kind_is_tokens() {
    // Default budget must be Budget::Tokens with a very large limit.
    let budget = DoubleAgent::budget();
    assert_eq!(budget.kind(), crux::types::budget::BudgetKind::Tokens);
    assert_eq!(budget.limit(), u64::MAX);
}

#[tokio::test]
async fn conformance_agent_handwritten_on_low_confidence_returns_continue() {
    let recovery = DoubleAgent::on_low_confidence(0.3);
    assert!(
        matches!(recovery, Recovery::Continue),
        "default on_low_confidence must return Recovery::Continue"
    );
}

#[tokio::test]
async fn conformance_agent_handwritten_on_step_failure_returns_propagate() {
    let err = CruxErr::step_failed("test_step", "test error");
    let recovery = DoubleAgent::on_step_failure(&err);
    assert!(
        matches!(recovery, Recovery::Propagate),
        "default on_step_failure must return Recovery::Propagate"
    );
}
