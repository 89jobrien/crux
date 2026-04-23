/// Shared test helpers for constructing `Step` and `Crux<T>` values.
///
/// Enabled via the `test-utils` feature. Intended for use in `#[cfg(test)]`
/// blocks and integration test crates across the workspace.
use chrono::Utc;

use crate::crux_value::Crux;
use crate::id::CruxId;
use crate::step::{Step, StepKind, StepStatus};

/// Build a plain, successful `Step` with the given name, input hash, and output.
pub fn step_ok(name: &str, input_hash: u64, output: Option<serde_json::Value>) -> Step {
    Step {
        name: name.into(),
        kind: StepKind::Plain,
        status: StepStatus::Ok,
        confidence: 1.0,
        started_at: Utc::now(),
        duration_ms: 0,
        input_hash,
        content_hash: None,
        output,
        error: None,
        attempt: 1,
        events: vec![],
    }
}

/// Build a plain, successful `Step` with an explicit `content_hash`.
pub fn step_with_content(
    name: &str,
    input_hash: u64,
    content_hash: Option<u64>,
    output: Option<serde_json::Value>,
) -> Step {
    Step {
        content_hash,
        ..step_ok(name, input_hash, output)
    }
}

/// Build a minimal successful `Crux<T>` with the given agent name, value, and steps.
pub fn crux_ok<T>(agent: &str, value: T, steps: Vec<Step>) -> Crux<T> {
    Crux {
        id: CruxId::new(),
        agent: agent.into(),
        value: Ok(value),
        steps,
        children: vec![],
        started_at: Utc::now(),
        finished_at: Some(Utc::now()),
    }
}
