//! Typed step lifecycle and streaming events.
//!
//! These replace the untyped `events: Vec<serde_json::Value>` on `Step`.
//! They are emitted by `StepRecorder` into the `EventPipeline` and can be
//! consumed by observers without touching the trace directly.
use serde::{Deserialize, Serialize};

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
