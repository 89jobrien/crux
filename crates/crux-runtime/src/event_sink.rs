//! Event sink adapters for ordered runtime events.

#[cfg(feature = "tracing")]
use crux_types::emission::{EventSink, RuntimeEvent};

/// Forwards ordered runtime events to the `tracing` ecosystem.
#[cfg(feature = "tracing")]
pub struct TracingEventSink;

#[cfg(feature = "tracing")]
impl EventSink for TracingEventSink {
    fn emit(&self, event: RuntimeEvent) {
        tracing::info!(
            sequence = event.sequence,
            trace_id = ?event.trace_id,
            agent = ?event.agent,
            emission = ?event.emission,
            "runtime.event"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::context::Context as _;
    use crate::ctx::CruxCtx;
    use crate::types::error::CruxErr;
    use crux_domain::pipeline::EventPipeline;
    use crux_types::emission::{Emission, RuntimeEvent, RuntimeEventFilter};

    #[cfg(feature = "tracing")]
    #[test]
    fn tracing_event_sink_accepts_ordered_events() {
        use crux_types::emission::EventSink as _;

        super::TracingEventSink.emit(RuntimeEvent {
            sequence: 0,
            emitted_at: chrono::Utc::now(),
            trace_id: None,
            agent: None,
            emission: Emission::StepStart { name: "x".into() },
        });
    }

    #[test]
    fn trace_jsonl_exports_step_span_fields_and_metadata() {
        let mut ctx = CruxCtx::new("agent");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(ctx.step("compile", || async { Ok::<_, CruxErr>(1) }))
            .unwrap();
        let mut trace = ctx.finalize(Ok::<_, CruxErr>(1));
        trace.steps[0]
            .metadata
            .insert("handler".into(), serde_json::json!("shell::run"));

        let jsonl = crate::observability::trace_to_jsonl(&trace).unwrap();
        let row: RuntimeEvent = serde_json::from_str(jsonl.trim()).unwrap();

        assert_eq!(row.sequence, 0);
        assert_eq!(row.trace_id.as_ref(), Some(&trace.id));
        assert_eq!(row.agent.as_deref(), Some("agent"));
        assert!(matches!(
            row.emission,
            Emission::StepRecorded {
                ref name,
                status: crate::types::step::StepStatus::Ok,
                origin: crate::types::step::StepOrigin::Live,
                ref metadata,
                ..
            } if name == "compile" && metadata["handler"] == "shell::run"
        ));
    }

    #[test]
    fn durable_event_log_replays_append_order() {
        let path = std::env::temp_dir().join(format!(
            "crux-events-{}.jsonl",
            crate::types::id::CruxId::new()
        ));
        let log = Arc::new(crate::event_log::EventLog::open(&path));
        let pipeline = EventPipeline::with_sinks(64, vec![log.clone()]);
        let mut receiver = pipeline.subscribe();
        let sender = pipeline.sender();
        sender.emit(None, None, Emission::ReplayHit { name: "a".into() });
        sender.emit(
            None,
            None,
            Emission::StepComplete {
                name: "a".into(),
                duration_ms: 1,
            },
        );
        receiver.try_recv().unwrap();
        receiver.try_recv().unwrap();

        let replayed = log.replay().unwrap();
        assert_eq!(replayed[0].sequence, 0);
        assert_eq!(replayed[1].sequence, 1);
        assert!(matches!(
            replayed[1].emission,
            Emission::StepComplete { .. }
        ));
        let replay_only = log
            .replay_filtered(&RuntimeEventFilter {
                replay_only: true,
                step_name: None,
            })
            .unwrap();
        assert_eq!(replay_only.len(), 1);
        assert!(matches!(
            replay_only[0].emission,
            Emission::ReplayHit { .. }
        ));
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn ctx_emits_started_event_on_step() {
        let pipeline = EventPipeline::new(64);
        let mut rx = pipeline.subscribe();

        let mut ctx = CruxCtx::new("agent");
        ctx.set_event_sender(pipeline.sender());

        ctx.step("my_step", || async { Ok::<i32, CruxErr>(1) })
            .await
            .unwrap();

        let replay_miss = rx.recv().await.unwrap();
        assert!(matches!(replay_miss.emission, Emission::ReplayMiss { .. }));
        let ev = rx.recv().await.unwrap();
        assert!(
            matches!(ev.emission, Emission::StepStart { ref name } if name == "my_step"),
            "expected Started, got: {ev:?}"
        );
        assert_eq!(ev.sequence, 1);
    }

    #[tokio::test]
    async fn ctx_emits_completed_event_after_ok_step() {
        let pipeline = EventPipeline::new(64);
        let mut rx = pipeline.subscribe();

        let mut ctx = CruxCtx::new("agent");
        ctx.set_event_sender(pipeline.sender());

        ctx.step("done_step", || async { Ok::<(), CruxErr>(()) })
            .await
            .unwrap();

        // Drain ReplayMiss and Started
        let _ = rx.recv().await.unwrap();
        let _ = rx.recv().await.unwrap();
        let ev = rx.recv().await.unwrap();
        assert!(
            matches!(ev.emission, Emission::StepComplete { ref name, .. } if name == "done_step"),
            "expected Completed, got: {ev:?}"
        );
    }

    #[tokio::test]
    async fn ctx_emits_failed_event_on_step_error() {
        let pipeline = EventPipeline::new(64);
        let mut rx = pipeline.subscribe();

        let mut ctx = CruxCtx::new("agent");
        ctx.set_event_sender(pipeline.sender());

        let _ = ctx
            .step("bad_step", || async {
                Err::<i32, _>(CruxErr::step_failed("bad_step", "boom"))
            })
            .await;

        // Drain ReplayMiss and Started
        let _ = rx.recv().await.unwrap();
        let _ = rx.recv().await.unwrap();
        let ev = rx.recv().await.unwrap();
        assert!(
            matches!(ev.emission, Emission::StepError { ref name, .. } if name == "bad_step"),
            "expected Failed, got: {ev:?}"
        );
    }

    #[tokio::test]
    async fn emit_step_event_sends_chunk() {
        let pipeline = EventPipeline::new(64);
        let mut rx = pipeline.subscribe();

        let mut ctx = CruxCtx::new("agent");
        ctx.set_event_sender(pipeline.sender());

        ctx.emit_step_event("my_step", serde_json::json!({"delta": "hi"}));

        let ev = rx.recv().await.unwrap();
        assert!(
            matches!(ev.emission, Emission::StepChunk { ref name, .. } if name == "my_step"),
            "expected Chunk, got: {ev:?}"
        );
    }

    #[tokio::test]
    async fn replay_diagnostics_share_the_ordered_runtime_stream() {
        let mut original = CruxCtx::new("agent");
        original
            .step("cached", || async { Ok::<_, CruxErr>(42) })
            .await
            .unwrap();
        let previous = original.finalize(Ok::<_, CruxErr>(serde_json::json!(42)));

        let pipeline = EventPipeline::new(64);
        let mut receiver = pipeline.subscribe();
        let mut replay = CruxCtx::new("agent");
        replay.set_event_sender(pipeline.sender());
        replay.replay_from(&previous);
        let value = replay
            .step("cached", || async { Ok::<_, CruxErr>(0) })
            .await
            .unwrap();

        assert_eq!(value, 42);
        let event = receiver.recv().await.unwrap();
        assert_eq!(event.sequence, 0);
        assert!(matches!(
            event.emission,
            Emission::ReplayHit { ref name } if name == "cached"
        ));
    }
}
