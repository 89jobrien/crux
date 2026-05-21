//! EventSink — port for emitting step events from CruxCtx.
#[cfg(test)]
mod tests {
    use crate::context::Context as _;
    use crate::ctx::CruxCtx;
    use crate::types::error::CruxErr;
    use crux_domain::event::StepEvent;
    use crux_domain::pipeline::EventPipeline;

    #[tokio::test]
    async fn ctx_emits_started_event_on_step() {
        let pipeline = EventPipeline::new(64);
        let mut rx = pipeline.subscribe();

        let mut ctx = CruxCtx::new("agent");
        ctx.set_event_sender(pipeline.sender());

        ctx.step("my_step", || async { Ok::<i32, CruxErr>(1) })
            .await
            .unwrap();

        let ev = rx.recv().await.unwrap();
        assert!(
            matches!(ev, StepEvent::Started { ref step_name } if step_name == "my_step"),
            "expected Started, got: {ev:?}"
        );
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

        // Drain Started
        let _ = rx.recv().await.unwrap();
        let ev = rx.recv().await.unwrap();
        assert!(
            matches!(ev, StepEvent::Completed { ref step_name, .. } if step_name == "done_step"),
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

        // Drain Started
        let _ = rx.recv().await.unwrap();
        let ev = rx.recv().await.unwrap();
        assert!(
            matches!(ev, StepEvent::Failed { ref step_name, .. } if step_name == "bad_step"),
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
            matches!(ev, StepEvent::Chunk { ref step_name, .. } if step_name == "my_step"),
            "expected Chunk, got: {ev:?}"
        );
    }
}
