/// Conformance tests: Context port — CruxCtx adapter.
///
/// Verifies CruxCtx satisfies the observable clauses of the Context trait
/// via the public agent API: step recording, step count, budget tracking,
/// snapshot steps, confidence, and error propagation.
use cruxai::prelude::*;

// ── step records in trace ────────────────────────────────────────────────────

#[tokio::test]
async fn conformance_context_step_appends_to_trace() {
    #[cruxai::agent]
    async fn single_step_agent() -> Crux<u32> {
        x.step("compute", || async { Ok::<u32, CruxErr>(42) }).await?;
        Ok(0)
    }

    let crux = single_step_agent().await;
    assert!(
        crux.steps.iter().any(|s| s.name == "compute"),
        "step named 'compute' must appear in trace"
    );
}

#[tokio::test]
async fn conformance_context_multiple_steps_all_recorded() {
    #[cruxai::agent]
    async fn three_step_agent() -> Crux<u32> {
        x.step("a", || async { Ok::<u32, CruxErr>(1) }).await?;
        x.step("b", || async { Ok::<u32, CruxErr>(2) }).await?;
        x.step("c", || async { Ok::<u32, CruxErr>(3) }).await?;
        Ok(0)
    }

    let crux = three_step_agent().await;
    assert_eq!(crux.steps.len(), 3, "three steps must be recorded");
}

// ── step result propagates to caller ─────────────────────────────────────────

#[tokio::test]
async fn conformance_context_step_result_returned_to_caller() {
    #[cruxai::agent]
    async fn value_agent() -> Crux<String> {
        let v = x
            .step("produce", || async { Ok::<String, CruxErr>("hello".into()) })
            .await?;
        Ok(v)
    }

    let crux = value_agent().await;
    assert_eq!(crux.value().unwrap(), "hello");
}

// ── step failure propagates ──────────────────────────────────────────────────

#[tokio::test]
async fn conformance_context_step_error_propagates_as_crux_err() {
    #[cruxai::agent]
    async fn failing_agent() -> Crux<u32> {
        x.step("boom", || async {
            Err::<u32, CruxErr>(CruxErr::step_failed("boom", "intentional"))
        })
        .await?;
        Ok(0)
    }

    let crux = failing_agent().await;
    assert!(crux.value().is_err(), "step error must propagate via Crux<T>");
}

// ── default budget is effectively unlimited ───────────────────────────────────

#[tokio::test]
async fn conformance_context_default_budget_is_unlimited() {
    #[cruxai::agent]
    async fn budget_probe() -> Crux<u64> {
        Ok(x.remaining_budget())
    }

    let crux = budget_probe().await;
    let remaining = *crux.value().unwrap();
    assert!(
        remaining > 1_000_000,
        "default budget must be effectively unlimited, got {remaining}"
    );
}

// ── step_with_confidence records the score ────────────────────────────────────

#[tokio::test]
async fn conformance_context_step_with_confidence_records_score() {
    #[cruxai::agent]
    async fn confident_agent() -> Crux<u32> {
        x.step_with_confidence("scored", 0.9, || async { Ok::<u32, CruxErr>(1) })
            .await?;
        Ok(0)
    }

    let crux = confident_agent().await;
    let step = crux
        .steps
        .iter()
        .find(|s| s.name == "scored")
        .expect("step 'scored' must be in trace");
    assert!(
        (step.confidence - 0.9).abs() < 1e-5,
        "confidence must be recorded as 0.9, got {}",
        step.confidence
    );
}

// ── finished_at is set after successful run ───────────────────────────────────

#[tokio::test]
async fn conformance_context_finished_at_set_on_completion() {
    #[cruxai::agent]
    async fn done_agent() -> Crux<u32> {
        Ok(1)
    }

    let crux = done_agent().await;
    assert!(crux.finished_at.is_some(), "finished_at must be set after run");
}
