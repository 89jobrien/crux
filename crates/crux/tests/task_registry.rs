/// Integration tests for TaskRegistry + replay lifecycle.
use cruxai::prelude::*;
use cruxai::registry::InMemoryBackend;

// -- Simple agent for testing -------------------------------------------------

#[cruxai::agent]
async fn adder(n: i32) -> Crux<i32> {
    let a: i32 = x
        .step("add_ten", || {
            let v = n;
            async move { Ok(v + 10) }
        })
        .await?;
    let b: i32 = x
        .step("add_five", || {
            let v = a;
            async move { Ok(v + 5) }
        })
        .await?;
    Ok(b)
}

// -- TaskRegistry lifecycle: submit, run, checkpoint, resume ------------------

#[tokio::test]
async fn full_task_lifecycle() {
    let reg = TaskRegistry::new(InMemoryBackend::new());

    // Submit a task.
    let id = reg.submit("add", serde_json::json!(7)).await.unwrap();
    let task = reg.get(&id).await.unwrap();
    assert_eq!(task.status, TaskStatus::Pending);

    // Mark running.
    reg.update_status(&id, TaskStatus::Running).await.unwrap();

    // Execute agent.
    let crux = adder(7).await;
    assert_eq!(*crux.value().unwrap(), 22); // 7 + 10 + 5

    // Checkpoint.
    reg.checkpoint(&id, &crux).await.unwrap();

    // Mark done.
    reg.update_status(&id, TaskStatus::Done).await.unwrap();

    let final_task = reg.get(&id).await.unwrap();
    assert_eq!(final_task.status, TaskStatus::Done);
    assert!(final_task.checkpoint.is_some());
    assert_eq!(final_task.attempts, 1);
}

#[tokio::test]
async fn resume_from_checkpoint_replays_steps() {
    let reg = TaskRegistry::new(InMemoryBackend::new());
    let id = reg.submit("add", serde_json::json!(3)).await.unwrap();

    // First run.
    let crux = adder(3).await;
    assert_eq!(*crux.value().unwrap(), 18); // 3 + 10 + 5
    reg.checkpoint(&id, &crux).await.unwrap();

    // Load checkpoint and replay.
    let snapshot = reg.load_checkpoint(&id).await.unwrap().unwrap();

    let mut ctx = CruxCtx::new("adder");
    ctx.replay_from(&snapshot);
    // Both steps should replay from cache (same name + hash).
    let a: i32 = ctx
        .step("add_ten", || async { panic!("should replay") })
        .await
        .unwrap();
    assert_eq!(a, 13); // 3 + 10
    let b: i32 = ctx
        .step("add_five", || async { panic!("should replay") })
        .await
        .unwrap();
    assert_eq!(b, 18); // 13 + 5
}

#[tokio::test]
async fn lenient_replay_skips_removed_step() {
    // First run has three steps: add_ten, transform, add_five.
    let mut ctx1 = CruxCtx::new("agent");
    let _: i32 = ctx1.step("add_ten", || async { Ok(10) }).await.unwrap();
    let _: i32 = ctx1.step("transform", || async { Ok(20) }).await.unwrap();
    let _: i32 = ctx1.step("add_five", || async { Ok(25) }).await.unwrap();
    let crux1 = ctx1.finalize(Ok(25));
    let snapshot = crux1.to_snapshot().unwrap();

    // Second run removes "transform" step. In lenient mode, "add_five" at ordinal 1
    // should scan forward and find the cached entry.
    let mut ctx2 = CruxCtx::new("agent");
    ctx2.replay_from(&snapshot);
    ctx2.set_replay_mode(ReplayMode::Lenient);

    let a: i32 = ctx2
        .step("add_ten", || async { panic!("should replay") })
        .await
        .unwrap();
    assert_eq!(a, 10);

    // "add_five" is at ordinal 1 but was cached at ordinal 2.
    // Lenient by-name scan finds it.
    let b: i32 = ctx2
        .step("add_five", || async { panic!("should replay") })
        .await
        .unwrap();
    assert_eq!(b, 25);
}

#[tokio::test]
async fn strict_replay_rejects_different_step_name() {
    let mut ctx1 = CruxCtx::new("agent");
    let _: i32 = ctx1.step("fetch", || async { Ok(1) }).await.unwrap();
    let crux1 = ctx1.finalize(Ok(1));
    let snapshot = crux1.to_snapshot().unwrap();

    let mut ctx2 = CruxCtx::new("agent");
    ctx2.replay_from(&snapshot);
    // Strict mode (default): different name at same ordinal => mismatch.
    let result: Result<i32, _> = ctx2.step("different_name", || async { Ok(1) }).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("replay mismatch"));
}

#[tokio::test]
async fn pending_returns_only_pending_tasks() {
    let reg = TaskRegistry::new(InMemoryBackend::new());
    let id1 = reg.submit("build", serde_json::json!(1)).await.unwrap();
    let _id2 = reg.submit("build", serde_json::json!(2)).await.unwrap();
    let _id3 = reg.submit("build", serde_json::json!(3)).await.unwrap();

    reg.update_status(&id1, TaskStatus::Done).await.unwrap();

    let pending = reg.pending("").await.unwrap();
    assert_eq!(pending.len(), 2);
}
