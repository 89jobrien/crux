/// StepRecorder — records steps into the execution trace.
///
/// Single responsibility: step construction and trace accumulation.
/// No hook invocation, no replay, no budget logic.
// TODO(#73): redaction before persist — apply a Redactor port before appending
//   output/error fields so sensitive data never enters the trace (cf. braid-redact)
use chrono::{DateTime, Utc};

use std::collections::HashMap;

use crate::types::step::{Step, StepKind, StepStatus};

/// Parameters for recording a step outcome.
pub struct StepRecord<'a> {
    pub name: &'a str,
    pub input_hash: u64,
    pub content_hash: Option<u64>,
    pub confidence: f32,
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub attempt: u32,
}

#[derive(Debug, Clone, Default)]
pub struct StepRecorder {
    steps: Vec<Step>,
    ordinal: u32,
}

impl StepRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate the next ordinal and return it with the input hash.
    pub fn next_ordinal(&mut self, name: &str) -> (u32, u64) {
        let ordinal = self.ordinal;
        self.ordinal += 1;
        (ordinal, hash_step_identity(name, ordinal))
    }

    /// Peek at the current ordinal without advancing it.
    pub fn current_ordinal(&self) -> u32 {
        self.ordinal
    }

    /// Record a successful step.
    pub fn record_ok(&mut self, rec: &StepRecord<'_>, output: Option<serde_json::Value>) {
        self.steps.push(Step {
            name: rec.name.to_string(),
            kind: StepKind::Plain,
            status: StepStatus::Ok,
            confidence: rec.confidence,
            started_at: rec.started_at,
            duration_ms: rec.duration_ms,
            input_hash: rec.input_hash,
            content_hash: rec.content_hash,
            output,
            error: None,
            attempt: rec.attempt,
            events: vec![],
            metadata: HashMap::new(),
            findings: vec![],
        });
    }

    /// Record a failed step.
    pub fn record_err(&mut self, rec: &StepRecord<'_>, error: &str) {
        self.steps.push(Step {
            name: rec.name.to_string(),
            kind: StepKind::Plain,
            status: StepStatus::Err,
            confidence: rec.confidence,
            started_at: rec.started_at,
            duration_ms: rec.duration_ms,
            input_hash: rec.input_hash,
            content_hash: rec.content_hash,
            output: None,
            error: Some(error.to_string()),
            attempt: rec.attempt,
            events: vec![],
            metadata: HashMap::new(),
            findings: vec![],
        });
    }

    /// Record a skipped step.
    pub fn record_skipped(&mut self, name: &str, input_hash: u64, confidence: f32) {
        self.steps.push(Step {
            name: name.to_string(),
            kind: StepKind::Plain,
            status: StepStatus::Skipped,
            confidence,
            started_at: Utc::now(),
            duration_ms: 0,
            input_hash,
            content_hash: None,
            output: None,
            error: None,
            attempt: 0,
            events: vec![],
            metadata: HashMap::new(),
            findings: vec![],
        });
    }

    /// Record a replayed step (attempt = 0).
    pub fn record_replay(
        &mut self,
        name: &str,
        input_hash: u64,
        content_hash: Option<u64>,
        confidence: f32,
        output: serde_json::Value,
    ) {
        self.steps.push(Step {
            name: name.to_string(),
            kind: StepKind::Plain,
            status: StepStatus::Ok,
            confidence,
            started_at: Utc::now(),
            duration_ms: 0,
            input_hash,
            content_hash,
            output: Some(output),
            error: None,
            attempt: 0,
            events: vec![],
            metadata: HashMap::new(),
            findings: vec![],
        });
    }

    /// Push a pre-built step directly (used by delegation/speculation).
    pub(crate) fn push_raw(&mut self, step: Step) {
        self.steps.push(step);
    }

    /// Get the last recorded step's output, if any.
    pub fn last_output(&self) -> Option<&serde_json::Value> {
        self.steps.last().and_then(|s| s.output.as_ref())
    }

    /// Get the last recorded step's error message, if any.
    pub fn last_error(&self) -> Option<&str> {
        self.steps.last().and_then(|s| s.error.as_deref())
    }

    /// View all recorded steps.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// Mutable access to steps (for patching events on streaming steps).
    pub(crate) fn steps_mut(&mut self) -> &mut Vec<Step> {
        &mut self.steps
    }

    /// Consume the recorder and return its steps.
    pub fn into_steps(self) -> Vec<Step> {
        self.steps
    }
}

/// Hash arbitrary serializable content for replay identity.
///
/// This produces an ordinal-independent hash suitable for the `content_hash`
/// field, allowing lenient replay to distinguish steps that share a name but
/// differ in actual input.
pub fn hash_content(value: &impl serde::Serialize) -> u64 {
    use std::hash::{Hash, Hasher};
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

pub fn hash_step_identity(name: &str, ordinal: u32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    ordinal.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinals_increment() {
        let mut recorder = StepRecorder::new();
        let (o1, h1) = recorder.next_ordinal("a");
        let (o2, h2) = recorder.next_ordinal("b");
        assert_eq!(o1, 0);
        assert_eq!(o2, 1);
        assert_ne!(h1, h2);
    }

    fn rec(name: &str, hash: u64) -> StepRecord<'_> {
        StepRecord {
            name,
            input_hash: hash,
            content_hash: None,
            confidence: 1.0,
            started_at: Utc::now(),
            duration_ms: 5,
            attempt: 1,
        }
    }

    #[test]
    fn record_ok_and_err() {
        let mut recorder = StepRecorder::new();
        recorder.record_ok(&rec("a", 0), None);
        recorder.record_err(&rec("b", 1), "fail");

        assert_eq!(recorder.steps().len(), 2);
        assert!(recorder.steps()[0].is_ok());
        assert!(recorder.steps()[1].is_err());
    }

    #[test]
    fn last_output_and_error() {
        let mut recorder = StepRecorder::new();
        recorder.record_ok(&rec("a", 0), Some(serde_json::json!(42)));
        assert_eq!(recorder.last_output(), Some(&serde_json::json!(42)));

        recorder.record_err(&rec("b", 1), "boom");
        assert_eq!(recorder.last_error(), Some("boom"));
    }
}
