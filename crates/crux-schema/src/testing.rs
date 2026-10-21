//! Test helpers for constructing schema values.

use chrono::Utc;
use crux_types::id::CruxId;
use crux_types::step::Step;

use crate::Crux;

/// Build a minimal successful `Crux<T>` with the given agent name, value, and steps.
pub fn crux_ok<T>(agent: &str, value: T, steps: Vec<Step>) -> Crux<T> {
    Crux {
        id: CruxId::new(),
        agent: agent.into(),
        pipeline_version: None,
        value: Ok(value),
        steps,
        children: vec![],
        started_at: Utc::now(),
        finished_at: Some(Utc::now()),
    }
}
