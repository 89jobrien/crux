//! Conformance test suite for RegistryBackend.
//!
//! Every impl of RegistryBackend must satisfy these invariants.
//! Call `assert_backend_contract(backend)` for each adapter.

use crux_runtime::registry::RegistryBackend;
use crux_types::id::TaskId;

async fn assert_backend_contract<B: RegistryBackend>(backend: B) {
    // 1. get on missing key returns None
    let missing = TaskId::new();
    assert_eq!(
        backend.get(&missing).await.unwrap(),
        None,
        "get on missing key must return None"
    );

    // 2. put then get returns the data
    let id = TaskId::new();
    backend.put(&id, b"hello".to_vec()).await.unwrap();
    assert_eq!(
        backend.get(&id).await.unwrap(),
        Some(b"hello".to_vec()),
        "get after put must return stored data"
    );

    // 3. put overwrites existing data
    backend.put(&id, b"world".to_vec()).await.unwrap();
    assert_eq!(
        backend.get(&id).await.unwrap(),
        Some(b"world".to_vec()),
        "put must overwrite existing data"
    );

    // 4. list returns all keys matching prefix
    let id2 = TaskId::new();
    backend.put(&id2, b"second".to_vec()).await.unwrap();
    let ids = backend.list("task_").await.unwrap();
    assert!(
        ids.len() >= 2,
        "list must return all keys with matching prefix"
    );
    assert!(
        ids.contains(&id) && ids.contains(&id2),
        "list must include all inserted keys"
    );

    // 5. list with non-matching prefix returns empty
    let empty = backend.list("nonexistent_").await.unwrap();
    assert!(
        empty.is_empty(),
        "list with non-matching prefix must return empty"
    );

    // 6. cas succeeds when expected matches current
    let current = backend.get(&id).await.unwrap().unwrap();
    let swapped = backend
        .cas(&id, current, b"cas_new".to_vec())
        .await
        .unwrap();
    assert!(swapped, "cas must succeed when expected matches current");
    assert_eq!(
        backend.get(&id).await.unwrap(),
        Some(b"cas_new".to_vec()),
        "cas must update the value on success"
    );

    // 7. cas fails when expected does not match current
    let swapped = backend
        .cas(&id, b"wrong".to_vec(), b"should_not_appear".to_vec())
        .await
        .unwrap();
    assert!(
        !swapped,
        "cas must fail when expected does not match current"
    );
    assert_eq!(
        backend.get(&id).await.unwrap(),
        Some(b"cas_new".to_vec()),
        "cas must not modify value on failure"
    );
}

// -- InMemoryBackend --

#[tokio::test]
async fn in_memory_satisfies_contract() {
    let backend = crux_runtime::registry::InMemoryBackend::new();
    assert_backend_contract(backend).await;
}

// -- SqliteBackend --

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_satisfies_contract() {
    let backend = crux_task::sqlite::SqliteBackend::open(":memory:").unwrap();
    assert_backend_contract(backend).await;
}

// -- RedbBackend --

#[cfg(feature = "redb")]
#[tokio::test]
async fn redb_satisfies_contract() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.redb");
    let backend = crux_runtime::registry::RedbBackend::open(path.to_str().unwrap()).unwrap();
    assert_backend_contract(backend).await;
}
