/// ReplayCache — stores and matches cached step outputs from a prior trace.
///
/// Single responsibility: replay matching. Given an ordinal and input_hash,
/// returns a cache hit, mismatch error, or miss.
use crate::types::crux_value::Crux;
use crate::types::error::CruxErr;

#[derive(Debug, Clone)]
struct ReplayEntry {
    input_hash: u64,
    output: Option<serde_json::Value>,
}

/// Result of checking the replay cache for a given step.
pub enum ReplayResult {
    /// Cache hit — return the cached output without re-executing.
    Hit(serde_json::Value),
    /// The step at this ordinal has a different hash — trace diverged.
    Mismatch { expected: u64, actual: u64 },
    /// No cached entry at this ordinal — execute normally.
    Miss,
}

#[derive(Debug, Clone, Default)]
pub struct ReplayCache {
    entries: Vec<ReplayEntry>,
    enabled: bool,
}

impl ReplayCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed replay from a previous trace.
    pub fn seed_from(&mut self, previous: &Crux<serde_json::Value>) {
        self.entries = previous
            .steps
            .iter()
            .map(|s| ReplayEntry {
                input_hash: s.input_hash,
                output: s.output.clone(),
            })
            .collect();
        self.enabled = true;
    }

    /// Check the cache for a step at the given ordinal with the given hash.
    pub fn check(&self, ordinal: u32, input_hash: u64) -> ReplayResult {
        if !self.enabled {
            return ReplayResult::Miss;
        }

        match self.entries.get(ordinal as usize) {
            Some(entry) if entry.input_hash == input_hash => match &entry.output {
                Some(output) => ReplayResult::Hit(output.clone()),
                None => ReplayResult::Miss,
            },
            Some(entry) => ReplayResult::Mismatch {
                expected: entry.input_hash,
                actual: input_hash,
            },
            None => ReplayResult::Miss,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl std::fmt::Debug for ReplayResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hit(_) => write!(f, "ReplayResult::Hit(...)"),
            Self::Mismatch { expected, actual } => {
                write!(f, "ReplayResult::Mismatch({expected} vs {actual})")
            }
            Self::Miss => write!(f, "ReplayResult::Miss"),
        }
    }
}

/// Deserialize a replay hit into the target type.
pub fn deserialize_replay<T: serde::de::DeserializeOwned>(
    name: &str,
    cached: serde_json::Value,
) -> Result<T, CruxErr> {
    serde_json::from_value(cached)
        .map_err(|e| CruxErr::step_failed(name, format!("replay deserialize: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::crux_value::Crux;
    use crate::types::id::CruxId;
    use crate::types::step::{Step, StepKind, StepStatus};
    use chrono::Utc;

    fn make_snapshot(steps: Vec<Step>) -> Crux<serde_json::Value> {
        Crux {
            id: CruxId::new(),
            agent: "test".into(),
            value: Ok(serde_json::json!(null)),
            steps,
            children: vec![],
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
        }
    }

    fn make_step(name: &str, input_hash: u64, output: Option<serde_json::Value>) -> Step {
        Step {
            name: name.into(),
            kind: StepKind::Plain,
            status: StepStatus::Ok,
            confidence: 1.0,
            started_at: Utc::now(),
            duration_ms: 0,
            input_hash,
            output,
            error: None,
            attempt: 1,
        }
    }

    #[test]
    fn hit_returns_cached_output() {
        let mut cache = ReplayCache::new();
        let snapshot = make_snapshot(vec![make_step("a", 42, Some(serde_json::json!("hello")))]);
        cache.seed_from(&snapshot);

        match cache.check(0, 42) {
            ReplayResult::Hit(val) => assert_eq!(val, serde_json::json!("hello")),
            other => panic!("expected Hit, got {other:?}"),
        }
    }

    #[test]
    fn mismatch_on_wrong_hash() {
        let mut cache = ReplayCache::new();
        let snapshot = make_snapshot(vec![make_step("a", 42, Some(serde_json::json!("x")))]);
        cache.seed_from(&snapshot);

        match cache.check(0, 99) {
            ReplayResult::Mismatch { expected, actual } => {
                assert_eq!(expected, 42);
                assert_eq!(actual, 99);
            }
            other => panic!("expected Mismatch, got {other:?}"),
        }
    }

    #[test]
    fn miss_past_cache_end() {
        let mut cache = ReplayCache::new();
        let snapshot = make_snapshot(vec![make_step("a", 42, Some(serde_json::json!("x")))]);
        cache.seed_from(&snapshot);

        assert!(matches!(cache.check(1, 99), ReplayResult::Miss));
    }

    #[test]
    fn miss_when_disabled() {
        let cache = ReplayCache::new();
        assert!(matches!(cache.check(0, 42), ReplayResult::Miss));
    }
}
