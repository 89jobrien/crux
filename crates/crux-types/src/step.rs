/// A single recorded step in an agent's execution.
use std::collections::HashMap;

// TODO(#94): streaming step subscriptions — formalize `events: Vec<Value>` as a
//   broadcast channel (Step::events_subscribe()) for real-time trace consumption
//   without waiting for step completion (cf. romp)

// TODO(#95): cited findings on failures — add a `cited_reason` field with source
//   traceability (file, symbol, line) for richer failure diagnostics (cf. devloop)

// TODO(#96): step output type safety — step outputs are all `Value` today; explore
//   generic `Step<T>` or a typed-output registry to catch mismatches at compile time

/// Shared mutable output map for `pipe()` stages — maps alias names to their outputs.
pub type StepState = HashMap<String, serde_json::Value>;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A structured diagnostic finding with optional source citation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitedFinding {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub name: String,
    pub kind: StepKind,
    pub status: StepStatus,
    pub confidence: f32,
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub input_hash: u64,
    pub content_hash: Option<u64>,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub attempt: u32,
    /// Intermediate events emitted during streaming steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<serde_json::Value>,
    /// Arbitrary per-step metadata for extensibility.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Structured diagnostic findings attached during execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<CitedFinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Plain,
    Delegation,
    Branch,
    Speculation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Ok,
    Err,
    Rejected,
    Skipped,
}

impl Step {
    pub fn is_ok(&self) -> bool {
        self.status == StepStatus::Ok
    }

    pub fn is_err(&self) -> bool {
        self.status == StepStatus::Err
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_kind_serializes_snake_case() {
        let json = serde_json::to_string(&StepKind::Delegation).unwrap();
        assert_eq!(json, "\"delegation\"");
    }

    #[test]
    fn step_status_serializes_snake_case() {
        let json = serde_json::to_string(&StepStatus::Ok).unwrap();
        assert_eq!(json, "\"ok\"");
    }
}
