//! End-to-end test: Planner + EventPipeline together as the agentic substrate.
use cruxx::prelude::*;
use cruxx_domain::event::StepEvent;
use cruxx_domain::pipeline::EventPipeline;
use cruxx_domain::planner::{DenyAllPlanner, SimulatePlanner};

#[tokio::test]
async fn deny_planner_blocks_all_steps_end_to_end() {
    let mut ctx = CruxCtx::new("agent");
    ctx.set_planner(DenyAllPlanner {
        reason: "policy".into(),
    });

    let result = ctx.step("fetch", || async { Ok::<i32, CruxErr>(1) }).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("policy"));
}

#[tokio::test]
async fn simulate_planner_returns_value_without_side_effects() {
    let mut ctx = CruxCtx::new("agent");
    ctx.set_planner(SimulatePlanner {
        output: serde_json::json!(42),
    });

    let result = ctx
        .step("expensive_step", || async {
            panic!("should not run");
            #[allow(unreachable_code)]
            Ok::<i32, CruxErr>(0)
        })
        .await;

    assert_eq!(result.unwrap(), 42i32);
}

#[tokio::test]
async fn event_pipeline_receives_all_step_lifecycle_events() {
    let pipeline = EventPipeline::new(128);
    let mut rx = pipeline.subscribe();

    let mut ctx = CruxCtx::new("agent");
    ctx.set_event_sender(pipeline.sender());

    ctx.step("step_a", || async { Ok::<i32, CruxErr>(1) })
        .await
        .unwrap();
    ctx.step("step_b", || async { Ok::<i32, CruxErr>(2) })
        .await
        .unwrap();

    let events: Vec<StepEvent> = (0..4).map(|_| rx.try_recv().unwrap()).collect();
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            StepEvent::Started { .. } => "started",
            StepEvent::Completed { .. } => "completed",
            _ => "other",
        })
        .collect();

    assert_eq!(kinds, ["started", "completed", "started", "completed"]);
}

#[tokio::test]
async fn planner_and_pipeline_compose() {
    // Passthrough planner + event pipeline work together
    let pipeline = EventPipeline::new(64);
    let mut rx = pipeline.subscribe();

    let mut ctx = CruxCtx::new("agent");
    ctx.set_event_sender(pipeline.sender());
    // Default passthrough planner — no set_planner call needed

    let result = ctx
        .step("compute", || async { Ok::<String, CruxErr>("done".into()) })
        .await;

    assert!(result.is_ok());
    let ev = rx.recv().await.unwrap();
    assert!(matches!(ev, StepEvent::Started { .. }));
}
