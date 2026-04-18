/// Integration tests for t.delegate::<A>() and DelegationBuilder.
mod common;

use common::{DoublerAgent, FailerAgent};
use cruxai::prelude::*;

// -- Basic delegation -------------------------------------------------------

#[cruxai::agent]
async fn parent_basic(n: i32) -> Crux<i32> {
    let result = t.delegate::<DoublerAgent>("double_it", n).run().await?;
    Ok(result)
}

#[tokio::test]
async fn basic_delegation() {
    let crux = parent_basic(21).await;
    assert_eq!(crux.value().unwrap(), &42);

    let delegation_steps: Vec<_> = crux
        .steps
        .iter()
        .filter(|s| s.kind == StepKind::Delegation)
        .collect();
    assert_eq!(delegation_steps.len(), 1);
    assert_eq!(delegation_steps[0].name, "double_it");

    assert_eq!(crux.children.len(), 1);
    assert_eq!(crux.children[0].agent, "doubler");
}

// -- Delegation failure wraps in CruxErr::Delegation ------------------------

#[cruxai::agent]
async fn parent_fail(input: String) -> Crux<String> {
    let result = t.delegate::<FailerAgent>("will_fail", input).run().await?;
    Ok(result)
}

#[tokio::test]
async fn delegation_failure() {
    let crux = parent_fail("test".to_string()).await;
    assert!(crux.value().is_err());

    let err = crux.value().unwrap_err();
    match err {
        CruxErr::Delegation { to, .. } => assert_eq!(to, "failer"),
        other => panic!("expected Delegation error, got: {other}"),
    }
}

// -- Delegation with budget -------------------------------------------------

#[cruxai::agent]
async fn parent_budgeted(n: i32) -> Crux<i32> {
    let result = t
        .delegate::<DoublerAgent>("budgeted", n)
        .with_budget(Budget::tokens(1000))
        .run()
        .await?;
    Ok(result)
}

#[tokio::test]
async fn delegation_with_budget() {
    let crux = parent_budgeted(5).await;
    assert_eq!(crux.value().unwrap(), &10);
}

// -- Delegation records in delegations() ------------------------------------

#[tokio::test]
async fn delegation_appears_in_delegations() {
    let crux = parent_basic(7).await;
    let delegations = crux.delegations();
    assert_eq!(delegations.len(), 1);
    assert_eq!(delegations[0].from_agent, "parent_basic");
    assert_eq!(delegations[0].to_agent, "doubler");
}

// -- Agent::run direct still works with delegate ----------------------------

#[tokio::test]
async fn delegate_from_manual_ctx() {
    let mut ctx = CruxCtx::new("manual");
    let result = ctx.delegate::<DoublerAgent>("manual_del", 3).run().await;
    assert_eq!(result.unwrap(), 6);
    assert_eq!(ctx.snapshot_steps().len(), 1);
}
