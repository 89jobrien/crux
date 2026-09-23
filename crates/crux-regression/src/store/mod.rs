mod file;
mod memory;

use crux_improve::Crux;
use serde_json::Value;

use crate::{BaselineRef, RegressionCaseId, RegressionError, RegressionReport, TraceDigest};

pub use file::FileRegressionStore;
pub use memory::InMemoryRegressionStore;

/// Persistence port for immutable traces, active baselines, and evaluation history.
pub trait RegressionStore: Send + Sync {
    /// Stores a trace idempotently and returns its content digest.
    fn put_trace(&self, trace: &Crux<Value>) -> Result<TraceDigest, RegressionError>;

    /// Loads and verifies a trace artifact by digest.
    fn trace(&self, digest: &TraceDigest) -> Result<Crux<Value>, RegressionError>;

    /// Atomically replaces a baseline only when its current digest matches `expected`.
    fn compare_and_set_baseline(
        &self,
        case: &RegressionCaseId,
        expected: Option<&TraceDigest>,
        next: &BaselineRef,
    ) -> Result<(), RegressionError>;

    /// Loads the active baseline reference for a case.
    fn baseline(&self, case: &RegressionCaseId) -> Result<BaselineRef, RegressionError>;

    /// Persists a report without replacing an existing report ID.
    fn append_report(&self, report: &RegressionReport) -> Result<(), RegressionError>;

    /// Returns reports in newest-first order.
    fn reports(&self, case: &RegressionCaseId) -> Result<Vec<RegressionReport>, RegressionError>;
}
