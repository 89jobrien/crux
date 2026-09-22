//! Serializable step records, statuses, kinds, state, and cited findings.

/// A single recorded step in an agent's execution.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};

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

/// Source location supporting a failure reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCitation {
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// Failure explanation paired with a precise source citation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitedReason {
    pub reason: String,
    pub source: SourceCitation,
}

/// A recorded step whose output type is checked at compile time.
///
/// Dynamic pipeline and plugin callers retain the wire-compatible
/// `serde_json::Value` output through the default type parameter.
///
/// ```compile_fail
/// use crux_types::step::{Step, StepKind, StepStatus};
/// # use chrono::Utc;
/// # use std::collections::HashMap;
/// let _: Step<u32> = Step {
///     stable_id: None,
///     name: "typed".into(),
///     kind: StepKind::Plain,
///     status: StepStatus::Ok,
///     confidence: 1.0,
///     started_at: Utc::now(),
///     duration_ms: 0,
///     input_hash: 0,
///     content_hash: None,
///     output: Some("not a number"),
///     error: None,
///     cited_reason: None,
///     attempt: 1,
///     events: vec![],
///     event_subscribers: Default::default(),
///     metadata: HashMap::new(),
///     findings: vec![],
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step<T = serde_json::Value> {
    /// Stable identity used by strict replay independently of trace position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_id: Option<String>,
    pub name: String,
    pub kind: StepKind,
    pub status: StepStatus,
    pub confidence: f32,
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub input_hash: u64,
    pub content_hash: Option<u64>,
    pub output: Option<T>,
    pub error: Option<String>,
    /// Traceable explanation for a failed step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cited_reason: Option<CitedReason>,
    pub attempt: u32,
    /// Intermediate events emitted during streaming steps.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<serde_json::Value>,
    /// Live subscribers for intermediate events. This runtime-only state is not serialized.
    #[doc(hidden)]
    #[serde(skip)]
    pub event_subscribers: Arc<Mutex<Vec<mpsc::Sender<serde_json::Value>>>>,
    /// Arbitrary per-step metadata for extensibility.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Structured diagnostic findings attached during execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<CitedFinding>,
}

/// A step paired with its strongly typed input channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedStep<I, O> {
    pub input: I,
    pub step: Step<O>,
}

/// Runtime-schema fallback used by YAML pipelines and plugin handlers.
pub type DynamicStep = TypedStep<serde_json::Value, serde_json::Value>;

impl<I, O> TypedStep<I, O> {
    pub fn new(input: I, step: Step<O>) -> Self {
        Self { input, step }
    }
}

impl<I, O> TypedStep<I, O>
where
    I: serde::de::DeserializeOwned,
    O: serde::de::DeserializeOwned,
{
    /// Validate dynamic input and output values against Rust channel types.
    pub fn try_from_dynamic(dynamic: DynamicStep) -> Result<Self, serde_json::Error> {
        Ok(Self {
            input: serde_json::from_value(dynamic.input)?,
            step: dynamic.step.try_map_output(serde_json::from_value)?,
        })
    }
}

impl<I, O> TypedStep<I, O>
where
    I: Serialize,
    O: Serialize,
{
    /// Erase Rust channel types for YAML or plugin transport.
    pub fn try_into_dynamic(self) -> Result<DynamicStep, serde_json::Error> {
        Ok(DynamicStep {
            input: serde_json::to_value(self.input)?,
            step: self.step.try_map_output(serde_json::to_value)?,
        })
    }
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

impl<T> Step<T> {
    /// Reports whether the step completed successfully.
    pub fn is_ok(&self) -> bool {
        self.status == StepStatus::Ok
    }

    /// Reports whether the step completed with an error.
    pub fn is_err(&self) -> bool {
        self.status == StepStatus::Err
    }

    /// Subscribe to intermediate events emitted after this call.
    pub fn events_subscribe(&self) -> mpsc::Receiver<serde_json::Value> {
        let (sender, receiver) = mpsc::channel();
        self.event_subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(sender);
        receiver
    }

    /// Record and immediately broadcast an intermediate event.
    pub fn emit_event(&mut self, event: serde_json::Value) {
        self.events.push(event.clone());
        self.event_subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|subscriber| subscriber.send(event.clone()).is_ok());
    }

    fn try_map_output<U, E>(self, map: impl FnOnce(T) -> Result<U, E>) -> Result<Step<U>, E> {
        Ok(Step {
            stable_id: self.stable_id,
            name: self.name,
            kind: self.kind,
            status: self.status,
            confidence: self.confidence,
            started_at: self.started_at,
            duration_ms: self.duration_ms,
            input_hash: self.input_hash,
            content_hash: self.content_hash,
            output: self.output.map(map).transpose()?,
            error: self.error,
            cited_reason: self.cited_reason,
            attempt: self.attempt,
            events: self.events,
            event_subscribers: self.event_subscribers,
            metadata: self.metadata,
            findings: self.findings,
        })
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

    #[test]
    fn typed_step_output_round_trips_without_json_erasure() {
        let step = Step::<u32> {
            stable_id: None,
            name: "count".into(),
            kind: StepKind::Plain,
            status: StepStatus::Ok,
            confidence: 1.0,
            started_at: Utc::now(),
            duration_ms: 1,
            input_hash: 0,
            content_hash: None,
            output: Some(42),
            error: None,
            cited_reason: None,
            attempt: 1,
            events: vec![],
            event_subscribers: Default::default(),
            metadata: HashMap::new(),
            findings: vec![],
        };

        let json = serde_json::to_string(&step).unwrap();
        let decoded: Step<u32> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.output, Some(42));
    }

    #[test]
    fn cited_failure_reason_round_trips_with_source_location() {
        let json = serde_json::json!({
            "name": "compile",
            "kind": "plain",
            "status": "err",
            "confidence": 1.0,
            "started_at": Utc::now(),
            "duration_ms": 1,
            "input_hash": 0,
            "content_hash": null,
            "output": null,
            "error": "compile failed",
            "attempt": 1,
            "cited_reason": {
                "reason": "type mismatch",
                "source": {
                    "file": "src/lib.rs",
                    "symbol": "build",
                    "line": 42
                }
            }
        });

        let step: Step = serde_json::from_value(json).unwrap();
        let encoded = serde_json::to_value(step).unwrap();
        assert_eq!(encoded["cited_reason"]["source"]["line"], 42);
    }

    #[test]
    fn event_subscribers_receive_events_as_they_are_emitted() {
        let mut step: Step = serde_json::from_value(serde_json::json!({
            "name": "stream",
            "kind": "plain",
            "status": "ok",
            "confidence": 1.0,
            "started_at": Utc::now(),
            "duration_ms": 0,
            "input_hash": 0,
            "content_hash": null,
            "output": null,
            "error": null,
            "attempt": 1
        }))
        .unwrap();
        let events = step.events_subscribe();

        step.emit_event(serde_json::json!({"token": "hello"}));

        assert_eq!(
            events.recv().unwrap(),
            serde_json::json!({"token": "hello"})
        );
        assert_eq!(step.events.len(), 1);
    }

    #[test]
    fn dynamic_channels_check_input_and_output_compatibility() {
        let dynamic: DynamicStep = serde_json::from_value(serde_json::json!({
            "input": "not a number",
            "step": {
                "name": "typed-edge",
                "kind": "plain",
                "status": "ok",
                "confidence": 1.0,
                "started_at": Utc::now(),
                "duration_ms": 0,
                "input_hash": 0,
                "content_hash": null,
                "output": "done",
                "error": null,
                "attempt": 1
            }
        }))
        .unwrap();

        let result = TypedStep::<u64, String>::try_from_dynamic(dynamic);

        assert!(result.is_err());
    }
}
