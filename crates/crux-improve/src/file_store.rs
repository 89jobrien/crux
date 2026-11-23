//! JSON file adapter for the [`ProposalStore`] port.
//!
//! [`JsonFileProposalStore`] keeps every proposal in one JSON array on disk.
//! Each mutation is written to a sibling temporary file and renamed over the
//! target, so a failed write leaves the previous file intact.

use std::path::{Path, PathBuf};

use crate::{
    CruxId,
    proposal::ProposalLifecycle,
    store::{ProposalStore, ProposalStoreError},
};

/// Durable [`ProposalStore`] backed by a single JSON file.
#[derive(Debug)]
pub struct JsonFileProposalStore {
    path: PathBuf,
    records: Vec<ProposalLifecycle>,
}

impl JsonFileProposalStore {
    /// Opens the store at `path`. A missing file yields an empty store; the
    /// file is created on the first mutation.
    ///
    /// # Errors
    ///
    /// Returns [`ProposalStoreError::Backend`] when the file exists but cannot
    /// be read or does not contain a valid proposal array.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, ProposalStoreError> {
        let path = path.into();
        let records = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| backend(format!("invalid proposal file {}", path.display()), e))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                return Err(backend(format!("failed to read {}", path.display()), e));
            }
        };
        Ok(Self { path, records })
    }

    /// Path of the backing file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn position(&self, id: &CruxId) -> Option<usize> {
        self.records.iter().position(|record| &record.id == id)
    }

    /// Writes `records` to a sibling temporary file and renames it over the
    /// backing file, so readers never observe a partially written array.
    fn persist(&self, records: &[ProposalLifecycle]) -> Result<(), ProposalStoreError> {
        let bytes = serde_json::to_vec_pretty(records)
            .map_err(|e| backend("failed to serialize proposals", e))?;
        let mut tmp = self.path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, &bytes)
            .map_err(|e| backend(format!("failed to write {}", tmp.display()), e))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|e| backend(format!("failed to replace {}", self.path.display()), e))
    }
}

fn backend(context: impl std::fmt::Display, cause: impl std::fmt::Display) -> ProposalStoreError {
    ProposalStoreError::Backend(format!("{context}: {cause}"))
}

impl ProposalStore for JsonFileProposalStore {
    fn create(&mut self, proposal: ProposalLifecycle) -> Result<(), ProposalStoreError> {
        if self.position(&proposal.id).is_some() {
            return Err(ProposalStoreError::AlreadyExists(proposal.id.to_string()));
        }
        let mut next = self.records.clone();
        next.push(proposal);
        self.persist(&next)?;
        self.records = next;
        Ok(())
    }

    fn update(&mut self, proposal: ProposalLifecycle) -> Result<(), ProposalStoreError> {
        let Some(index) = self.position(&proposal.id) else {
            return Err(ProposalStoreError::NotFound(proposal.id.to_string()));
        };
        let mut next = self.records.clone();
        next[index] = proposal;
        self.persist(&next)?;
        self.records = next;
        Ok(())
    }

    fn get(&self, id: &CruxId) -> Result<Option<ProposalLifecycle>, ProposalStoreError> {
        Ok(self.position(id).map(|index| self.records[index].clone()))
    }

    fn list(&self) -> Result<Vec<ProposalLifecycle>, ProposalStoreError> {
        Ok(self.records.clone())
    }

    fn delete(&mut self, id: &CruxId) -> Result<(), ProposalStoreError> {
        let Some(index) = self.position(id) else {
            return Ok(());
        };
        let mut next = self.records.clone();
        next.remove(index);
        self.persist(&next)?;
        self.records = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::{Improvement, ImprovementKind, StrategyDiff, proposal::ProposalStatus};

    /// Unique scratch path under the system temp dir, removed on drop.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("crux-improve-{}.json", CruxId::new())))
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let mut tmp = self.0.as_os_str().to_owned();
            tmp.push(".tmp");
            let _ = std::fs::remove_file(PathBuf::from(tmp));
        }
    }

    fn proposal() -> ProposalLifecycle {
        ProposalLifecycle::new(Improvement {
            id: CruxId::new(),
            kind: ImprovementKind::PromptTemplate,
            target: "system".into(),
            diff: StrategyDiff::default(),
            confidence: 0.8,
            evidence: vec![],
            proposed_at: Utc::now(),
        })
    }

    #[test]
    fn open_missing_file_yields_empty_store() {
        let file = TempFile::new();
        let store = JsonFileProposalStore::open(&file.0).unwrap();
        assert!(store.list().unwrap().is_empty());
        assert!(!file.0.exists(), "opening must not create the file");
    }

    #[test]
    fn create_persists_across_reopen() {
        let file = TempFile::new();
        let p = proposal();
        let id = p.id.clone();
        {
            let mut store = JsonFileProposalStore::open(&file.0).unwrap();
            store.create(p).unwrap();
        }
        let reopened = JsonFileProposalStore::open(&file.0).unwrap();
        let got = reopened
            .get(&id)
            .unwrap()
            .expect("proposal survives reopen");
        assert_eq!(got.id, id);
    }

    #[test]
    fn create_duplicate_is_rejected() {
        let file = TempFile::new();
        let mut store = JsonFileProposalStore::open(&file.0).unwrap();
        let p = proposal();
        store.create(p.clone()).unwrap();
        assert!(matches!(
            store.create(p),
            Err(ProposalStoreError::AlreadyExists(_))
        ));
    }

    #[test]
    fn update_persists_lifecycle_history_across_reopen() {
        let file = TempFile::new();
        let mut p = proposal();
        let id = p.id.clone();
        {
            let mut store = JsonFileProposalStore::open(&file.0).unwrap();
            store.create(p.clone()).unwrap();
            p.record_policy_validation(true, vec![]).unwrap();
            store.update(p.clone()).unwrap();
        }
        let reopened = JsonFileProposalStore::open(&file.0).unwrap();
        let got = reopened.get(&id).unwrap().unwrap();
        assert_eq!(got.status, ProposalStatus::ValidationPassed);
        assert_eq!(got.history.len(), 2);
    }

    #[test]
    fn update_missing_proposal_is_not_found() {
        let file = TempFile::new();
        let mut store = JsonFileProposalStore::open(&file.0).unwrap();
        assert!(matches!(
            store.update(proposal()),
            Err(ProposalStoreError::NotFound(_))
        ));
    }

    #[test]
    fn delete_persists_removal_across_reopen() {
        let file = TempFile::new();
        let p = proposal();
        let id = p.id.clone();
        {
            let mut store = JsonFileProposalStore::open(&file.0).unwrap();
            store.create(p).unwrap();
            store.delete(&id).unwrap();
        }
        let reopened = JsonFileProposalStore::open(&file.0).unwrap();
        assert!(reopened.get(&id).unwrap().is_none());
    }

    #[test]
    fn list_preserves_insertion_order_across_reopen() {
        let file = TempFile::new();
        let p1 = proposal();
        let p2 = proposal();
        let (id1, id2) = (p1.id.clone(), p2.id.clone());
        {
            let mut store = JsonFileProposalStore::open(&file.0).unwrap();
            store.create(p1).unwrap();
            store.create(p2).unwrap();
        }
        let listed = JsonFileProposalStore::open(&file.0)
            .unwrap()
            .list()
            .unwrap();
        assert_eq!(listed[0].id, id1);
        assert_eq!(listed[1].id, id2);
    }

    #[test]
    fn corrupt_file_is_a_backend_error() {
        let file = TempFile::new();
        std::fs::write(&file.0, "not json").unwrap();
        assert!(matches!(
            JsonFileProposalStore::open(&file.0),
            Err(ProposalStoreError::Backend(_))
        ));
    }
}
