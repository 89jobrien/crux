//! Budget and trace behavior for delegated pipeline agents.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crux_runtime::prelude::{Agent, Context, CruxCtx};
use crux_script::{HandlerRegistry, Runner};
use crux_types::error::CruxErr;
use crux_types::step::{StepKind, StepStatus};
use serde_json::Value;

struct TracedAgent;

impl Agent for TracedAgent {
    type Input = Value;
    type Output = Value;

    fn name() -> &'static str {
        "traced-agent"
    }

    async fn run(ctx: &mut CruxCtx, input: Value) -> Result<Value, CruxErr> {
        ctx.step("inside-agent", || async move { Ok(input) }).await
    }
}

#[tokio::test]
async fn delegate_enforces_scoped_budget_and_preserves_child_trace() {
    let calls = Arc::new(AtomicU32::new(0));
    let agent_calls = Arc::clone(&calls);
    let mut registry = HandlerRegistry::new();
    registry.agent_fn("bounded-agent", move |input: Value| {
        let agent_calls = Arc::clone(&agent_calls);
        async move {
            agent_calls.fetch_add(1, Ordering::SeqCst);
            Ok(input)
        }
    });
    let pipeline = crux_script::load(
        r#"
pipeline: scoped-delegation
steps:
  - delegate: bounded-agent
    name: bounded
    budget:
      steps: 0
"#,
    )
    .expect("valid pipeline");

    let trace = Runner::new(Arc::new(registry))
        .run(&pipeline, serde_json::json!({"value": 1}))
        .await;

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(
        matches!(trace.value().unwrap_err(), CruxErr::Delegation { .. }),
        "unexpected delegation error: {:?}",
        trace.value()
    );
    assert_eq!(trace.steps.len(), 1);
    assert_eq!(trace.steps[0].kind, StepKind::Delegation);
    assert_eq!(trace.steps[0].status, StepStatus::Err);
    assert_eq!(trace.children.len(), 1);
    assert_eq!(trace.children[0].agent, "bounded-agent");
    assert!(matches!(
        trace.children[0].value(),
        Err(CruxErr::StepBudgetExceeded {
            limit: 0,
            attempted: 1
        })
    ));
}

#[tokio::test]
async fn delegate_charges_parent_pipeline_budget() {
    let handler_calls = Arc::new(AtomicU32::new(0));
    let handled = Arc::clone(&handler_calls);
    let mut registry = HandlerRegistry::new();
    registry.agent::<TracedAgent>("one-call-agent");
    registry.handler_value_free("test::after", move |input: Value| {
        let handled = Arc::clone(&handled);
        async move {
            handled.fetch_add(1, Ordering::SeqCst);
            Ok(input)
        }
    });
    let pipeline = crux_script::load(
        r#"
pipeline: parent-budget
budget:
  steps: 1
steps:
  - delegate: one-call-agent
    name: delegated
    budget:
      steps: 1
  - step: after
    handler: test::after
"#,
    )
    .expect("valid pipeline");

    let trace = Runner::new(Arc::new(registry))
        .run(&pipeline, Value::Null)
        .await;

    assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        trace.value(),
        Err(CruxErr::StepBudgetExceeded {
            limit: 1,
            attempted: 2
        })
    ));
    assert_eq!(trace.children.len(), 1);
    assert_eq!(trace.children[0].agent, "one-call-agent");
    assert_eq!(trace.children[0].steps.len(), 1);
    assert_eq!(trace.children[0].steps[0].name, "inside-agent");
}

#[tokio::test]
async fn parent_budget_rejects_delegate_before_child_execution() {
    let calls = Arc::new(AtomicU32::new(0));
    let agent_calls = Arc::clone(&calls);
    let mut registry = HandlerRegistry::new();
    registry.agent_fn("blocked-agent", move |input: Value| {
        let agent_calls = Arc::clone(&agent_calls);
        async move {
            agent_calls.fetch_add(1, Ordering::SeqCst);
            Ok(input)
        }
    });
    let pipeline = crux_script::load(
        r#"
pipeline: blocked-delegation
budget:
  steps: 0
steps:
  - delegate: blocked-agent
    budget:
      steps: 1
"#,
    )
    .expect("valid pipeline");

    let trace = Runner::new(Arc::new(registry))
        .run(&pipeline, Value::Null)
        .await;

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        trace.value(),
        Err(CruxErr::StepBudgetExceeded {
            limit: 0,
            attempted: 1
        })
    ));
    assert!(trace.steps.is_empty());
    assert!(trace.children.is_empty());
}
