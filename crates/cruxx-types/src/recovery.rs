use serde::{Deserialize, Serialize};

/// Serializable subset of Recovery<T> — excludes closure variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryKind {
    Retry,
    Skip,
    Propagate,
    Continue,
}
