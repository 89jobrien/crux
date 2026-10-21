//! Golden trace regression storage and lifecycle orchestration.
#![warn(missing_docs)]

mod artifact;
mod error;
mod harness;
mod report;
mod store;

pub use artifact::{RegressionCaseId, RegressionRunId, TraceDigest, digest_trace};
pub use error::RegressionError;
pub use harness::RegressionHarness;
pub use report::{BaselineRef, RegressionReport};
pub use store::{FileRegressionStore, InMemoryRegressionStore, RegressionStore};
