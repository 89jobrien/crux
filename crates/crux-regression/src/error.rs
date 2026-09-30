use std::path::PathBuf;

use crux_improve::EvalConfigError;
use thiserror::Error;

use crate::{RegressionCaseId, RegressionRunId, TraceDigest};

/// Errors produced by regression artifact storage and evaluation.
#[derive(Debug, Error)]
pub enum RegressionError {
    /// A case identifier is empty or contains unsafe path characters.
    #[error("invalid regression case id '{0}'")]
    InvalidCaseId(String),
    /// A digest is not valid `sha256:<hex>` syntax.
    #[error("invalid trace digest '{0}'")]
    InvalidDigest(String),
    /// A report identifier is not a valid ULID.
    #[error("invalid regression run id '{0}'")]
    InvalidRunId(String),
    /// A stored envelope uses an unsupported format version.
    #[error("unsupported trace envelope version {0}")]
    UnsupportedFormatVersion(u16),
    /// A stored envelope names an unsupported digest algorithm.
    #[error("unsupported digest algorithm '{0}'")]
    UnsupportedDigestAlgorithm(String),
    /// The requested immutable trace artifact does not exist.
    #[error("trace artifact {0} was not found")]
    ArtifactNotFound(TraceDigest),
    /// The requested case has no active baseline.
    #[error("baseline for case {0} was not found")]
    BaselineNotFound(RegressionCaseId),
    /// A baseline file contains a different case than its path.
    #[error("baseline case mismatch: expected {expected}, got {actual}")]
    BaselineCaseMismatch {
        /// Case implied by the storage path.
        expected: RegressionCaseId,
        /// Case encoded in the baseline file.
        actual: RegressionCaseId,
    },
    /// An artifact's content does not match its requested digest.
    #[error("trace digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch {
        /// Digest used to address the artifact.
        expected: TraceDigest,
        /// Digest recomputed from stored content.
        actual: TraceDigest,
    },
    /// A baseline compare-and-set operation observed a stale value.
    #[error("baseline conflict for {case}: expected {expected:?}, got {actual:?}")]
    BaselineConflict {
        /// Case whose baseline was being changed.
        case: RegressionCaseId,
        /// Caller-supplied current digest.
        expected: Option<TraceDigest>,
        /// Digest observed under the store lock.
        actual: Option<TraceDigest>,
    },
    /// An append attempted to reuse an existing report ID.
    #[error("regression report {0} already exists")]
    ReportAlreadyExists(RegressionRunId),
    /// A persisted entry is malformed or inconsistent with its path.
    #[error("corrupt regression artifact at {path}: {message}")]
    CorruptArtifact {
        /// Path containing the malformed entry.
        path: PathBuf,
        /// Human-readable integrity failure.
        message: String,
    },
    /// An in-memory store mutex was poisoned.
    #[error("regression store lock was poisoned")]
    LockPoisoned,
    /// Filesystem access failed.
    #[error("regression I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization or deserialization failed.
    #[error("regression serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    /// Pure regression-policy validation or evaluation failed.
    #[error(transparent)]
    Evaluation(#[from] EvalConfigError),
}
