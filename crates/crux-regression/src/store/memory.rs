use std::{collections::BTreeMap, sync::Mutex};

use crux_improve::Crux;
use serde_json::Value;

use crate::{
    BaselineRef, RegressionCaseId, RegressionError, RegressionReport, RegressionRunId, TraceDigest,
    digest_trace,
};

use super::RegressionStore;

#[derive(Debug, Default)]
struct InMemoryStoreState {
    objects: BTreeMap<TraceDigest, Crux<Value>>,
    baselines: BTreeMap<RegressionCaseId, BaselineRef>,
    reports: BTreeMap<RegressionCaseId, BTreeMap<RegressionRunId, RegressionReport>>,
}

/// Thread-safe in-memory regression store for tests and ephemeral evaluation.
#[derive(Debug, Default)]
pub struct InMemoryRegressionStore {
    state: Mutex<InMemoryStoreState>,
}

impl RegressionStore for InMemoryRegressionStore {
    fn put_trace(&self, trace: &Crux<Value>) -> Result<TraceDigest, RegressionError> {
        let digest = digest_trace(trace)?;
        self.state
            .lock()
            .map_err(|_| RegressionError::LockPoisoned)?
            .objects
            .entry(digest.clone())
            .or_insert_with(|| trace.clone());
        Ok(digest)
    }

    fn trace(&self, digest: &TraceDigest) -> Result<Crux<Value>, RegressionError> {
        self.state
            .lock()
            .map_err(|_| RegressionError::LockPoisoned)?
            .objects
            .get(digest)
            .cloned()
            .ok_or_else(|| RegressionError::ArtifactNotFound(digest.clone()))
    }

    fn compare_and_set_baseline(
        &self,
        case: &RegressionCaseId,
        expected: Option<&TraceDigest>,
        next: &BaselineRef,
    ) -> Result<(), RegressionError> {
        if &next.case != case {
            return Err(RegressionError::BaselineCaseMismatch {
                expected: case.clone(),
                actual: next.case.clone(),
            });
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| RegressionError::LockPoisoned)?;
        if !state.objects.contains_key(&next.trace) {
            return Err(RegressionError::ArtifactNotFound(next.trace.clone()));
        }
        let actual = state.baselines.get(case).map(|baseline| &baseline.trace);
        if actual != expected {
            return Err(RegressionError::BaselineConflict {
                case: case.clone(),
                expected: expected.cloned(),
                actual: actual.cloned(),
            });
        }
        state.baselines.insert(case.clone(), next.clone());
        Ok(())
    }

    fn baseline(&self, case: &RegressionCaseId) -> Result<BaselineRef, RegressionError> {
        self.state
            .lock()
            .map_err(|_| RegressionError::LockPoisoned)?
            .baselines
            .get(case)
            .cloned()
            .ok_or_else(|| RegressionError::BaselineNotFound(case.clone()))
    }

    fn append_report(&self, report: &RegressionReport) -> Result<(), RegressionError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| RegressionError::LockPoisoned)?;
        if !state.objects.contains_key(&report.baseline) {
            return Err(RegressionError::ArtifactNotFound(report.baseline.clone()));
        }
        if !state.objects.contains_key(&report.candidate) {
            return Err(RegressionError::ArtifactNotFound(report.candidate.clone()));
        }
        let reports = state.reports.entry(report.case.clone()).or_default();
        if reports.contains_key(&report.id) {
            return Err(RegressionError::ReportAlreadyExists(report.id.clone()));
        }
        reports.insert(report.id.clone(), report.clone());
        Ok(())
    }

    fn reports(&self, case: &RegressionCaseId) -> Result<Vec<RegressionReport>, RegressionError> {
        let mut reports: Vec<RegressionReport> = self
            .state
            .lock()
            .map_err(|_| RegressionError::LockPoisoned)?
            .reports
            .get(case)
            .map(|reports| reports.values().rev().cloned().collect())
            .unwrap_or_default();
        reports.sort_by(|left: &RegressionReport, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(reports)
    }
}
