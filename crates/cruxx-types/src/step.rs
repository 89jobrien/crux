/// A single recorded step in an agent's execution.
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
