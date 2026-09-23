//! Typed step lifecycle and streaming events.
//!
//! These replace the untyped `events: Vec<serde_json::Value>` on `Step`.
//! They are emitted by `StepRecorder` into the `EventPipeline` and can be
//! consumed by observers without touching the trace directly.
use serde::{Deserialize, Serialize};

use crux_types::emission::Emission;

/// A typed event emitted during step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepEvent {
    /// Step execution has started.
    Started { step_name: String },
    /// An intermediate streaming chunk from a streaming step.
    Chunk {
        step_name: String,
        payload: serde_json::Value,
    },
    /// Step completed successfully.
    Completed { step_name: String, duration_ms: u64 },
    /// Step failed.
    Failed { step_name: String, error: String },
    /// Step was skipped by planner.
    Skipped { step_name: String, reason: String },
    /// Step was denied by planner.
    Denied { step_name: String, reason: String },
    /// Custom application event (escape hatch for domain-specific events).
    Custom {
        tag: String,
        payload: serde_json::Value,
    },
}

impl StepEvent {
    /// Converts this compatibility event into the canonical runtime emission.
    pub fn into_emission(self) -> Emission {
        match self {
            Self::Started { step_name } => Emission::StepStart { name: step_name },
            Self::Chunk { step_name, payload } => Emission::StepChunk {
                name: step_name,
                payload,
            },
            Self::Completed {
                step_name,
                duration_ms,
            } => Emission::StepComplete {
                name: step_name,
                duration_ms,
            },
            Self::Failed { step_name, error } => Emission::StepError {
                name: step_name,
                error,
            },
            Self::Skipped { step_name, reason } => Emission::StepSkipped {
                name: step_name,
                reason,
            },
            Self::Denied { step_name, reason } => Emission::StepDenied {
                name: step_name,
                reason,
            },
            Self::Custom { tag, payload } => Emission::Decision {
                source: "step_event".into(),
                key: tag,
                value: payload,
            },
        }
    }

    /// Return the associated step name for lifecycle events.
    pub fn step_name(&self) -> Option<&str> {
        match self {
            Self::Started { step_name }
            | Self::Chunk { step_name, .. }
            | Self::Completed { step_name, .. }
            | Self::Failed { step_name, .. }
            | Self::Skipped { step_name, .. }
            | Self::Denied { step_name, .. } => Some(step_name),
            Self::Custom { .. } => None,
        }
    }

    /// Whether this event closes a step lifecycle.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. }
                | Self::Failed { .. }
                | Self::Skipped { .. }
                | Self::Denied { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_events_expose_filtering_metadata() {
        let completed = StepEvent::Completed {
            step_name: "compile".into(),
            duration_ms: 12,
        };
        let custom = StepEvent::Custom {
            tag: "metric".into(),
            payload: serde_json::json!(1),
        };

        assert_eq!(completed.step_name(), Some("compile"));
        assert!(completed.is_terminal());
        assert_eq!(custom.step_name(), None);
        assert!(!custom.is_terminal());
    }
}
