use cruxx::prelude::TaskId;
/// Conformance tests: RegistryBackend port — InMemoryBackend adapter.
///
/// Verifies InMemoryBackend satisfies every clause of the RegistryBackend port
/// contract as a downstream consumer of the public API.
use cruxx::registry::{InMemoryBackend, RegistryBackend, TaskRegistry, TaskStatus};

// ── get/put round-trip ──────────────────────────────────────────────────────

#[tokio::test]
async fn conformance_registry_put_then_get_returns_same_bytes() {
    let b = InMemoryBackend::new();
    let id = TaskId::new();
    b.put(&id, b"payload".to_vec()).await.unwrap();
    assert_eq!(b.get(&id).await.unwrap(), Some(b"payload".to_vec()));
}

#[tokio::test]
async fn conformance_registry_get_absent_key_returns_none() {
    let b = InMemoryBackend::new();
    let id = TaskId::new();
    assert_eq!(b.get(&id).await.unwrap(), None);
}

#[tokio::test]
async fn conformance_registry_put_overwrites_previous_value() {
    let b = InMemoryBackend::new();
    let id = TaskId::new();
    b.put(&id, b"v1".to_vec()).await.unwrap();
    b.put(&id, b"v2".to_vec()).await.unwrap();
    assert_eq!(b.get(&id).await.unwrap(), Some(b"v2".to_vec()));
}

// ── list by prefix ──────────────────────────────────────────────────────────

#[tokio::test]
async fn conformance_registry_list_returns_all_inserted_keys() {
    let b = InMemoryBackend::new();
    let id1 = TaskId::new();
    let id2 = TaskId::new();
    let id3 = TaskId::new();
    b.put(&id1, b"a".to_vec()).await.unwrap();
    b.put(&id2, b"b".to_vec()).await.unwrap();
    b.put(&id3, b"c".to_vec()).await.unwrap();

    let all = b.list("").await.unwrap();
    assert_eq!(all.len(), 3, "list('') must return all 3 inserted keys");
}

#[tokio::test]
async fn conformance_registry_list_prefix_filters_by_prefix() {
    let b = InMemoryBackend::new();
    let id1 = TaskId::new();
    let id2 = TaskId::new();
    b.put(&id1, b"x".to_vec()).await.unwrap();
    b.put(&id2, b"y".to_vec()).await.unwrap();

    // A prefix matching exactly one key's first 8 chars should narrow results.
    let prefix = &id1.as_str()[..8];
    let subset = b.list(prefix).await.unwrap();
    assert!(
        subset.contains(&id1),
        "list with matching prefix must include id1"
    );
    // The other id should not appear unless it shares the same prefix (astronomically unlikely).
    if !id2.as_str().starts_with(prefix) {
        assert!(
            !subset.contains(&id2),
            "list with prefix must exclude non-matching id"
        );
    }
}

// ── CAS semantics ───────────────────────────────────────────────────────────

#[tokio::test]
async fn conformance_registry_cas_swaps_when_expected_matches() {
    let b = InMemoryBackend::new();
    let id = TaskId::new();
    b.put(&id, b"old".to_vec()).await.unwrap();
    let swapped = b.cas(&id, b"old".to_vec(), b"new".to_vec()).await.unwrap();
    assert!(swapped, "CAS must succeed when expected matches current");
    assert_eq!(b.get(&id).await.unwrap(), Some(b"new".to_vec()));
}

#[tokio::test]
async fn conformance_registry_cas_rejects_stale_expected() {
    let b = InMemoryBackend::new();
    let id = TaskId::new();
    b.put(&id, b"current".to_vec()).await.unwrap();
    let swapped = b
        .cas(&id, b"stale".to_vec(), b"new".to_vec())
        .await
        .unwrap();
    assert!(!swapped, "CAS must fail when expected does not match");
    assert_eq!(
        b.get(&id).await.unwrap(),
        Some(b"current".to_vec()),
        "value must be unchanged after failed CAS"
    );
}

#[tokio::test]
async fn conformance_registry_cas_rejects_on_missing_key() {
    let b = InMemoryBackend::new();
    let id = TaskId::new();
    let swapped = b.cas(&id, b"any".to_vec(), b"new".to_vec()).await.unwrap();
    assert!(!swapped, "CAS on absent key must return false");
}

// ── TaskRegistry high-level lifecycle ───────────────────────────────────────

#[tokio::test]
async fn conformance_registry_submit_creates_pending_task() {
    let reg = TaskRegistry::new(InMemoryBackend::new());
    let id = reg
        .submit("my_kind", serde_json::json!({"x": 1}))
        .await
        .unwrap();
    let task = reg.get(&id).await.unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert_eq!(task.kind, "my_kind");
    assert_eq!(task.attempts, 0);
}

#[tokio::test]
async fn conformance_registry_update_status_transitions_correctly() {
    let reg = TaskRegistry::new(InMemoryBackend::new());
    let id = reg.submit("job", serde_json::json!(null)).await.unwrap();
    reg.update_status(&id, TaskStatus::Running).await.unwrap();
    let task = reg.get(&id).await.unwrap();
    assert_eq!(task.status, TaskStatus::Running);
}

#[tokio::test]
async fn conformance_registry_pending_returns_only_pending_tasks() {
    let reg = TaskRegistry::new(InMemoryBackend::new());
    let id1 = reg.submit("job", serde_json::json!(1)).await.unwrap();
    let id2 = reg.submit("job", serde_json::json!(2)).await.unwrap();
    reg.update_status(&id1, TaskStatus::Done).await.unwrap();

    let pending = reg.pending("job").await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, id2);
}
