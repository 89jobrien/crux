//! cruxx-domain: Pure domain types for the cruxx agentic DSL.
//!
//! Zero async, zero LLM dependencies. External consumers (minibox, slash)
//! can depend on this crate without pulling tokio or BAML.

pub mod action;
pub mod event;
#[cfg(feature = "tokio-pipeline")]
pub mod pipeline;
pub mod plan_result;
pub mod planner;

#[cfg(test)]
mod tests {
    use crate::action::{Action, StepIntent};
    use crate::event::StepEvent;
    use crate::plan_result::PlanResult;

    use crate::planner::{PassthroughPlanner, Planner};

    #[test]
    fn domain_crate_compiles() {}

    #[test]
    fn action_execute_roundtrips_serde() {
        let a = Action::Execute(StepIntent {
            name: "my_step".into(),
            priority: 0,
        });
        let json = serde_json::to_string(&a).unwrap();
        let back: Action = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, Action::Execute(_)));
    }

    #[test]
    fn plan_result_allow_carries_action() {
        let a = Action::Execute(StepIntent {
            name: "x".into(),
            priority: 0,
        });
        let r = PlanResult::Allow(a.clone());
        assert!(matches!(r, PlanResult::Allow(_)));
    }

    #[test]
    fn plan_result_deny_carries_reason() {
        let r = PlanResult::Deny {
            reason: "unsafe".into(),
        };
        if let PlanResult::Deny { reason } = r {
            assert_eq!(reason, "unsafe");
        }
    }

    #[test]
    fn plan_result_simulate_carries_output() {
        let r = PlanResult::Simulate {
            output: serde_json::json!(42),
        };
        if let PlanResult::Simulate { output } = r {
            assert_eq!(output, serde_json::json!(42));
        }
    }

    #[test]
    fn passthrough_allows_all_steps() {
        let p = PassthroughPlanner;
        let result = p.next_action("my_step", 0);
        assert!(matches!(result, PlanResult::Allow(_)));
    }

    #[test]
    fn passthrough_preserves_step_name() {
        let p = PassthroughPlanner;
        if let PlanResult::Allow(action) = p.next_action("fetch_data", 0) {
            assert_eq!(action.name(), "fetch_data");
        } else {
            panic!("expected Allow");
        }
    }

    #[test]
    fn step_event_serializes_tag() {
        let e = StepEvent::Started {
            step_name: "fetch".into(),
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["kind"], "started");
        assert_eq!(json["step_name"], "fetch");
    }

    #[test]
    fn step_event_chunk_carries_payload() {
        let e = StepEvent::Chunk {
            payload: serde_json::json!({"token": "hello"}),
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["kind"], "chunk");
    }

    #[test]
    fn step_event_completed_carries_duration() {
        let e = StepEvent::Completed {
            step_name: "fetch".into(),
            duration_ms: 42,
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["duration_ms"], 42);
    }
}

#[cfg(all(test, feature = "tokio-pipeline"))]
mod pipeline_tests {
    use crate::event::StepEvent;
    use crate::pipeline::EventPipeline;

    #[tokio::test]
    async fn pipeline_delivers_event_to_subscriber() {
        let pipeline = EventPipeline::new(64);
        let mut rx = pipeline.subscribe();

        let sender = pipeline.sender();
        sender
            .send(StepEvent::Started {
                step_name: "test".into(),
            })
            .ok();

        let received = rx.recv().await.unwrap();
        assert!(matches!(received, StepEvent::Started { .. }));
    }

    #[tokio::test]
    async fn pipeline_drops_events_when_no_subscriber() {
        let pipeline = EventPipeline::new(64);
        let sender = pipeline.sender();
        // Sending with no subscriber should not panic or block
        let _ = sender.send(StepEvent::Started {
            step_name: "x".into(),
        });
    }

    #[tokio::test]
    async fn multiple_subscribers_each_receive_event() {
        let pipeline = EventPipeline::new(64);
        let mut rx1 = pipeline.subscribe();
        let mut rx2 = pipeline.subscribe();

        pipeline
            .sender()
            .send(StepEvent::Completed {
                step_name: "s".into(),
                duration_ms: 1,
            })
            .ok();

        assert!(matches!(
            rx1.recv().await.unwrap(),
            StepEvent::Completed { .. }
        ));
        assert!(matches!(
            rx2.recv().await.unwrap(),
            StepEvent::Completed { .. }
        ));
    }
}
