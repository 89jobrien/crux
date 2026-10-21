//! Port and in-memory adapter for persisting [`ProposalLifecycle`] records.
//!
//! The [`ProposalStore`] trait is the port.  [`InMemoryProposalStore`] is the
//! default in-process adapter, suitable for tests and single-process runtimes.
//! Production callers can implement [`ProposalStore`] for any durable backend
//! (SQLite, redb, remote KV, …) without touching domain code.

use std::collections::HashMap;

use thiserror::Error;

use crate::{CruxId, proposal::ProposalLifecycle};

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors returned by [`ProposalStore`] operations.
#[derive(Debug, Error, PartialEq)]
pub enum ProposalStoreError {
    #[error("proposal not found: {0}")]
    NotFound(String),
    #[error("a proposal with id {0} already exists")]
    AlreadyExists(String),
    #[error("store backend error: {0}")]
    Backend(String),
}

// ---------------------------------------------------------------------------
// Port
// ---------------------------------------------------------------------------

/// Persistence port for improvement proposal lifecycles.
///
/// Domain code depends only on this trait.  Infrastructure adapters implement
/// it for the chosen storage backend.
pub trait ProposalStore: Send + Sync {
    /// Persists a new proposal.
    ///
    /// # Errors
    ///
    /// Returns [`ProposalStoreError::AlreadyExists`] if a proposal with the
    /// same [`CruxId`] is already stored.
    fn create(&mut self, proposal: ProposalLifecycle) -> Result<(), ProposalStoreError>;

    /// Overwrites an existing proposal record.
    ///
    /// # Errors
    ///
    /// Returns [`ProposalStoreError::NotFound`] if no proposal with that id
    /// exists yet.
    fn update(&mut self, proposal: ProposalLifecycle) -> Result<(), ProposalStoreError>;

    /// Retrieves a proposal by id, returning `None` if absent.
    fn get(&self, id: &CruxId) -> Result<Option<ProposalLifecycle>, ProposalStoreError>;

    /// Returns all stored proposals in insertion order.
    fn list(&self) -> Result<Vec<ProposalLifecycle>, ProposalStoreError>;

    /// Removes a proposal.  Returns `Ok(())` when the record did not exist.
    fn delete(&mut self, id: &CruxId) -> Result<(), ProposalStoreError>;
}

// ---------------------------------------------------------------------------
// InMemoryProposalStore
// ---------------------------------------------------------------------------

/// In-memory adapter for [`ProposalStore`].
///
/// Stores proposals in a `HashMap` keyed by [`CruxId`] string and maintains a
/// separate insertion-order index so that [`list`](ProposalStore::list) is
/// deterministic.
#[derive(Debug, Default)]
pub struct InMemoryProposalStore {
    records: HashMap<String, ProposalLifecycle>,
    order: Vec<String>,
}

impl InMemoryProposalStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn key(id: &CruxId) -> String {
        id.to_string()
    }
}

impl ProposalStore for InMemoryProposalStore {
    fn create(&mut self, proposal: ProposalLifecycle) -> Result<(), ProposalStoreError> {
        let key = Self::key(&proposal.id);
        if self.records.contains_key(&key) {
            return Err(ProposalStoreError::AlreadyExists(key));
        }
        self.order.push(key.clone());
        self.records.insert(key, proposal);
        Ok(())
    }

    fn update(&mut self, proposal: ProposalLifecycle) -> Result<(), ProposalStoreError> {
        let key = Self::key(&proposal.id);
        if !self.records.contains_key(&key) {
            return Err(ProposalStoreError::NotFound(key));
        }
        self.records.insert(key, proposal);
        Ok(())
    }

    fn get(&self, id: &CruxId) -> Result<Option<ProposalLifecycle>, ProposalStoreError> {
        Ok(self.records.get(&Self::key(id)).cloned())
    }

    fn list(&self) -> Result<Vec<ProposalLifecycle>, ProposalStoreError> {
        let proposals = self
            .order
            .iter()
            .filter_map(|key| self.records.get(key).cloned())
            .collect();
        Ok(proposals)
    }

    fn delete(&mut self, id: &CruxId) -> Result<(), ProposalStoreError> {
        let key = Self::key(id);
        self.records.remove(&key);
        self.order.retain(|k| k != &key);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CruxId, Improvement, ImprovementKind, StrategyDiff, proposal::ProposalLifecycle};
    use chrono::Utc;

    fn make_proposal() -> ProposalLifecycle {
        let imp = Improvement {
            id: CruxId::new(),
            kind: ImprovementKind::PromptTemplate,
            target: "system".into(),
            diff: StrategyDiff::default(),
            confidence: 0.8,
            evidence: vec![],
            proposed_at: Utc::now(),
        };
        ProposalLifecycle::new(imp)
    }

    #[test]
    fn create_and_get_round_trip() {
        let mut store = InMemoryProposalStore::new();
        let p = make_proposal();
        let id = p.id.clone();
        store.create(p.clone()).unwrap();
        let retrieved = store.get(&id).unwrap().expect("should exist");
        assert_eq!(retrieved.id, id);
    }

    #[test]
    fn create_duplicate_is_rejected() {
        let mut store = InMemoryProposalStore::new();
        let p = make_proposal();
        store.create(p.clone()).unwrap();
        let err = store.create(p).unwrap_err();
        assert!(matches!(err, ProposalStoreError::AlreadyExists(_)));
    }

    #[test]
    fn update_persists_mutation() {
        let mut store = InMemoryProposalStore::new();
        let mut p = make_proposal();
        let id = p.id.clone();
        store.create(p.clone()).unwrap();

        p.record_policy_validation(true, vec![]).unwrap();
        p.accept().unwrap();
        store.update(p.clone()).unwrap();

        let retrieved = store.get(&id).unwrap().unwrap();
        assert!(matches!(
            retrieved.status,
            crate::proposal::ProposalStatus::Accepted { .. }
        ));
    }

    #[test]
    fn update_missing_proposal_is_not_found() {
        let mut store = InMemoryProposalStore::new();
        let p = make_proposal();
        let err = store.update(p).unwrap_err();
        assert!(matches!(err, ProposalStoreError::NotFound(_)));
    }

    #[test]
    fn list_preserves_insertion_order() {
        let mut store = InMemoryProposalStore::new();
        let p1 = make_proposal();
        let p2 = make_proposal();
        let id1 = p1.id.clone();
        let id2 = p2.id.clone();
        store.create(p1).unwrap();
        store.create(p2).unwrap();

        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, id1);
        assert_eq!(listed[1].id, id2);
    }

    #[test]
    fn delete_removes_record() {
        let mut store = InMemoryProposalStore::new();
        let p = make_proposal();
        let id = p.id.clone();
        store.create(p).unwrap();
        store.delete(&id).unwrap();
        assert!(store.get(&id).unwrap().is_none());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn delete_idempotent_for_missing_id() {
        let mut store = InMemoryProposalStore::new();
        let id = CruxId::new();
        store.delete(&id).unwrap(); // should not error
    }

    #[test]
    fn get_absent_returns_none() {
        let store = InMemoryProposalStore::new();
        let id = CruxId::new();
        assert!(store.get(&id).unwrap().is_none());
    }

    #[test]
    fn list_empty_store_returns_empty() {
        let store = InMemoryProposalStore::new();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn full_lifecycle_persisted_and_queryable() {
        let mut store = InMemoryProposalStore::new();
        let mut p = make_proposal();
        let id = p.id.clone();
        store.create(p.clone()).unwrap();

        // validate
        p.record_policy_validation(true, vec![]).unwrap();
        store.update(p.clone()).unwrap();

        // request approval
        p.request_approval().unwrap();
        store.update(p.clone()).unwrap();

        // approve
        p.decide_approval(crate::proposal::ApprovalDecision::Granted, "alice", None)
            .unwrap();
        store.update(p.clone()).unwrap();

        // rollback
        p.rollback(Some("latency".into())).unwrap();
        store.update(p.clone()).unwrap();

        let final_state = store.get(&id).unwrap().unwrap();
        assert!(matches!(
            final_state.status,
            crate::proposal::ProposalStatus::RolledBack { .. }
        ));
        // Created, ValidationRecorded, ApprovalRequired, ApprovalDecided, RolledBack = 5
        assert_eq!(final_state.history.len(), 5);
    }
}
