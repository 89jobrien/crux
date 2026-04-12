/// Integration tests for checkpoint/resume functionality.
use cruxai::prelude::*;
use cruxai::registry::{InMemoryBackend, TaskRegistry, TaskStatus};

#[cruxai::agent]
async fn checkpointable(steps: Vec<String>) -> Crux<String> {
    let mut result = String::new();
    for s in &steps {
        let part: String = t
            .step(s, || {
                let val = s.clone();
                async move { Ok(val) }
            })
            .await?;
        result.push_str(&part);
    }
    Ok(result)
}

#[tokio::test]
async fn snapshot_captures_in_progress_state() {
    let mut ctx = CruxCtx::new("test");
    let _: String = ctx
        .step("step_a", || async { Ok("a".to_string()) })
        .await
        .unwrap();
    let _: String = ctx
        .step("step_b", || async { Ok("b".to_string()) })
        .await
        .unwrap();

    let snapshot = ctx.snapshot();
    assert_eq!(snapshot.agent, "test");
    assert_eq!(snapshot.steps.len(), 2);
    assert!(snapshot.finished_at.is_none()); // not finalized
    assert_eq!(snapshot.steps[0].name, "step_a");
    assert_eq!(snapshot.steps[1].name, "step_b");
}

#[tokio::test]
async fn checkpoint_to_registry_and_resume() {
    let registry = TaskRegistry::new(InMemoryBackend::new());

    // Submit a task
    let task_id = registry
        .submit("compute", serde_json::json!("input"))
        .await
        .unwrap();
    registry
        .update_status(&task_id, TaskStatus::Running)
        .await
        .unwrap();

    // Simulate partial execution + checkpoint
    let mut ctx1 = CruxCtx::new("compute");
    let _: String = ctx1
        .step("step_1", || async { Ok("first".to_string()) })
        .await
        .unwrap();
    ctx1.checkpoint_to(&registry, &task_id).await.unwrap();

    // Verify checkpoint is stored
    let cp = registry.load_checkpoint(&task_id).await.unwrap();
    assert!(cp.is_some());
    let cp = cp.unwrap();
    assert_eq!(cp.steps.len(), 1);
    assert_eq!(cp.steps[0].name, "step_1");

    // Resume in a new context
    let mut ctx2 = CruxCtx::new("compute");
    ctx2.resume_from(&registry, &task_id).await.unwrap();

    // step_1 should replay from cache
    let val: String = ctx2
        .step("step_1", || async { Ok("first".to_string()) })
        .await
        .unwrap();
    assert_eq!(val, "first");

    // step_2 executes fresh
    let val2: String = ctx2
        .step("step_2", || async { Ok("second".to_string()) })
        .await
        .unwrap();
    assert_eq!(val2, "second");

    assert_eq!(ctx2.step_count(), 2);
}

#[tokio::test]
async fn resume_from_nonexistent_checkpoint_is_ok() {
    let registry = TaskRegistry::new(InMemoryBackend::new());
    let task_id = registry
        .submit("test", serde_json::json!(null))
        .await
        .unwrap();

    let mut ctx = CruxCtx::new("test");
    // No checkpoint stored — resume should succeed (no replay data)
    ctx.resume_from(&registry, &task_id).await.unwrap();

    // Fresh execution works
    let val: String = ctx
        .step("fresh", || async { Ok("ok".to_string()) })
        .await
        .unwrap();
    assert_eq!(val, "ok");
}
