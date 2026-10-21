//! Replay wire types — mode and result variants for deterministic step replay.

use serde::{Deserialize, Serialize};

/// Controls how the replay cache matches steps from a prior trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayMode {
    /// Ordinal-based lookup. Hash must match exactly or Mismatch is returned.
    #[default]
    Strict,
    /// By-name lookup with ordinal hint. If the name at the expected ordinal
    /// doesn't match, scans forward for the next entry with the same name.
    /// Hash mismatches return Miss (re-execute) instead of Mismatch (error).
    Lenient,
}

/// Result of checking the replay cache for a given step.
#[derive(Debug)]
pub enum ReplayResult {
    /// Cache hit — return the cached output without re-executing.
    Hit(serde_json::Value, f32),
    /// The step at this ordinal has a different hash — trace diverged.
    Mismatch { expected: u64, actual: u64 },
    /// No cached entry at this ordinal — execute normally.
    Miss,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_mode_default_is_strict() {
        assert_eq!(ReplayMode::default(), ReplayMode::Strict);
    }

    #[test]
    fn replay_mode_serde_round_trip() {
        for mode in [ReplayMode::Strict, ReplayMode::Lenient] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: ReplayMode = serde_json::from_str(&json).unwrap();
            assert_eq!(back, mode);
        }
    }

    #[test]
    fn replay_mode_strict_serializes_correctly() {
        let json = serde_json::to_string(&ReplayMode::Strict).unwrap();
        assert_eq!(json, "\"strict\"");
    }

    #[test]
    fn replay_mode_lenient_serializes_correctly() {
        let json = serde_json::to_string(&ReplayMode::Lenient).unwrap();
        assert_eq!(json, "\"lenient\"");
    }

    #[test]
    fn replay_result_debug_hit() {
        let r = ReplayResult::Hit(serde_json::json!("ok"), 0.9);
        let s = format!("{r:?}");
        assert!(s.contains("Hit"));
    }

    #[test]
    fn replay_result_debug_mismatch() {
        let r = ReplayResult::Mismatch {
            expected: 1,
            actual: 2,
        };
        let s = format!("{r:?}");
        assert!(s.contains("Mismatch"));
    }

    #[test]
    fn replay_result_debug_miss() {
        let r = ReplayResult::Miss;
        let s = format!("{r:?}");
        assert!(s.contains("Miss"));
    }
}

#[cfg(test)]
mod proptest_replay_mode {
    use super::*;
    use proptest::prelude::*;

    fn arb_replay_mode() -> impl Strategy<Value = ReplayMode> {
        prop_oneof![Just(ReplayMode::Strict), Just(ReplayMode::Lenient),]
    }

    proptest! {
        #[test]
        fn replay_mode_serde_identity(mode in arb_replay_mode()) {
            let json = serde_json::to_string(&mode).unwrap();
            let back: ReplayMode = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(back, mode);
        }
    }
}
