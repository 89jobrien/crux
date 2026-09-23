//! crux-domain: Pure domain types for the crux agentic DSL.
//!
//! Zero async, zero LLM dependencies. External consumers (minibox, slash)
//! can depend on this crate without pulling tokio or BAML.

pub mod action;
pub mod event;
pub mod phase;
#[cfg(feature = "tokio-pipeline")]
pub mod pipeline;
pub mod plan_result;
pub mod planner;

#[cfg(test)]
mod tests {
    use crate::action::{Action, StepIntent};
    use crate::event::StepEvent;
    use crate::phase::{ExecutionPhase, PhaseTransition};
    use crate::plan_result::PlanResult;

    use crate::planner::{PassthroughPlanner, Planner};

    #[test]
    fn domain_crate_compiles() {}

    #[test]
    fn execution_phases_enforce_architecture_order() {
        assert_eq!(
            ExecutionPhase::Planning.advance(ExecutionPhase::Policy),
            Ok(PhaseTransition {
                from: ExecutionPhase::Planning,
                to: ExecutionPhase::Policy,
            })
        );
        assert!(
            ExecutionPhase::Planning
                .advance(ExecutionPhase::Recording)
                .is_err()
        );
    }

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
            step_name: "test-step".into(),
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
    use std::sync::{Arc, Mutex};

    use crate::event::StepEvent;
    use crate::pipeline::EventPipeline;
    use crux_types::emission::{EventSink, RuntimeEvent};

    const TEST_CHANNEL_CAPACITY: usize = 64;

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<RuntimeEvent>>);

    impl EventSink for RecordingSink {
        fn emit(&self, event: RuntimeEvent) {
            self.0
                .lock()
                .expect("recording sink mutex should not be poisoned")
                .push(event);
        }
    }

    #[tokio::test]
    async fn pipeline_orders_sinks_and_subscribers_identically() {
        let sink = Arc::new(RecordingSink::default());
        let pipeline = EventPipeline::with_sinks(TEST_CHANNEL_CAPACITY, vec![sink.clone()]);
        let mut first = pipeline.subscribe();
        let mut second = pipeline.subscribe();
        let sender = pipeline.sender();

        sender
            .send(StepEvent::Started {
                step_name: "compile".into(),
            })
            .expect("subscribers should receive started event");
        sender
            .send(StepEvent::Completed {
                step_name: "compile".into(),
                duration_ms: 4,
            })
            .expect("subscribers should receive completed event");

        let first_events = [
            first.recv().await.expect("first subscriber event 0"),
            first.recv().await.expect("first subscriber event 1"),
        ];
        let second_events = [
            second.recv().await.expect("second subscriber event 0"),
            second.recv().await.expect("second subscriber event 1"),
        ];
        let sink_events = sink
            .0
            .lock()
            .expect("recording sink mutex should not be poisoned")
            .clone();

        assert_eq!(first_events[0].sequence, 0);
        assert_eq!(first_events[1].sequence, 1);
        assert_eq!(first_events.as_slice(), second_events.as_slice());
        assert_eq!(first_events.as_slice(), sink_events.as_slice());
    }

    #[tokio::test]
    async fn pipeline_delivers_event_to_subscriber() {
        let pipeline = EventPipeline::new(TEST_CHANNEL_CAPACITY);
        let mut rx = pipeline.subscribe();

        let sender = pipeline.sender();
        sender
            .send(StepEvent::Started {
                step_name: "test".into(),
            })
            .ok();

        let received = rx.recv().await.unwrap();
        assert!(matches!(
            received.emission,
            crux_types::emission::Emission::StepStart { .. }
        ));
    }

    #[tokio::test]
    async fn pipeline_drops_events_when_no_subscriber() {
        let pipeline = EventPipeline::new(TEST_CHANNEL_CAPACITY);
        let sender = pipeline.sender();
        // Sending with no subscriber should not panic or block
        let _ = sender.send(StepEvent::Started {
            step_name: "x".into(),
        });
    }

    #[tokio::test]
    async fn multiple_subscribers_each_receive_event() {
        let pipeline = EventPipeline::new(TEST_CHANNEL_CAPACITY);
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
            rx1.recv().await.unwrap().emission,
            crux_types::emission::Emission::StepComplete { .. }
        ));
        assert!(matches!(
            rx2.recv().await.unwrap().emission,
            crux_types::emission::Emission::StepComplete { .. }
        ));
    }
}
