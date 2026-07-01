//! End-to-end workflow test: create tasks with dependencies,
//! resolve them, verify ready/blocked queries.

use crux_runtime::registry::InMemoryBackend;
use crux_task::{ProjectTaskStatus, TaskManager, TaskPatch, TaskSpec};
use crux_types::task::Priority;

fn spec(title: &str, priority: Priority) -> TaskSpec {
    TaskSpec {
        title: title.into(),
        description: None,
        priority,
        status: ProjectTaskStatus::Open,
        labels: vec![],
        dependencies: vec![],
    }
}

#[tokio::test]
async fn full_workflow() {
    let mgr = TaskManager::new(InMemoryBackend::new());

    // Create a 3-task chain: setup -> implement -> test
    let setup = mgr.add(spec("Setup project", Priority::P0)).await.unwrap();
    let implement = mgr
        .add(spec("Implement feature", Priority::P1))
        .await
        .unwrap();
    let test = mgr.add(spec("Write tests", Priority::P1)).await.unwrap();

    // implement blocked by setup, test blocked by implement
    mgr.block(&implement, &setup).await.unwrap();
    mgr.block(&test, &implement).await.unwrap();

    // Only setup is ready
    let ready = mgr.ready().await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, setup);

    // blocked returns implement and test
    let blocked = mgr.blocked().await.unwrap();
    assert_eq!(blocked.len(), 2);

    // Complete setup -> implement becomes ready
    mgr.update(
        &setup,
        TaskPatch {
            status: Some(ProjectTaskStatus::Done),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let ready = mgr.ready().await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, implement);

    // Complete implement -> test becomes ready
    mgr.update(
        &implement,
        TaskPatch {
            status: Some(ProjectTaskStatus::Done),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let ready = mgr.ready().await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, test);

    // Stats
    let s = mgr.stats().await.unwrap();
    assert_eq!(s.total, 3);

    // by_priority returns P0 first
    let sorted = mgr.by_priority().await.unwrap();
    assert_eq!(sorted[0].spec.priority, Priority::P0);
}
