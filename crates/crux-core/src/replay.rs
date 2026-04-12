/// ReplayCache — stores and matches cached step outputs from a prior trace.
///
/// Single responsibility: replay matching. Given an ordinal, name, and input_hash,
/// returns a cache hit, mismatch error, or miss.
///
/// Supports two modes:
/// - Strict: ordinal-based lookup, hash must match exactly.
/// - Lenient: by-name lookup with ordinal fallback, skips missing intermediate steps.
use crate::types::crux_value::Crux;
use crate::types::error::CruxErr;

/// Controls how the replay cache matches steps from a prior trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplayMode {
    /// Ordinal-based lookup. Hash must match exactly or Mismatch is returned.
    #[default]
    Strict,
    /// By-name lookup with ordinal hint. If the name at the expected ordinal
    /// doesn't match, scans forward for the next entry with the same name.
    /// Hash mismatches return Miss (re-execute) instead of Mismatch (error).
    Lenient,
}

#[derive(Debug, Clone)]
struct ReplayEntry {
    name: String,
    input_hash: u64,
    content_hash: Option<u64>,
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
    mode: ReplayMode,
}

impl ReplayCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a replay cache with the given mode.
    pub fn with_mode(mode: ReplayMode) -> Self {
        Self {
            mode,
            ..Self::default()
        }
    }

    /// Set the replay mode.
    pub fn set_mode(&mut self, mode: ReplayMode) {
        self.mode = mode;
    }

    pub fn mode(&self) -> ReplayMode {
        self.mode
    }

    /// Seed replay from a previous trace.
    pub fn seed_from(&mut self, previous: &Crux<serde_json::Value>) {
        self.entries = previous
            .steps
            .iter()
            .map(|s| ReplayEntry {
                name: s.name.clone(),
                input_hash: s.input_hash,
                content_hash: s.content_hash,
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
            Some(entry) => match self.mode {
                ReplayMode::Strict => ReplayResult::Mismatch {
                    expected: entry.input_hash,
                    actual: input_hash,
                },
                ReplayMode::Lenient => ReplayResult::Miss,
            },
            None => ReplayResult::Miss,
        }
    }

    /// Check the cache using step name for lookup (lenient mode enhancement).
    ///
    /// In strict mode, delegates to ordinal-based `check`.
    /// In lenient mode, first tries ordinal match, then scans for a matching name.
    /// When `content_hash` is provided, the forward scan requires it to match,
    /// reducing false-positive cache hits for steps that share a name but differ
    /// in actual input.
    pub fn check_by_name(
        &self,
        name: &str,
        ordinal: u32,
        input_hash: u64,
        content_hash: Option<u64>,
    ) -> ReplayResult {
        if !self.enabled {
            return ReplayResult::Miss;
        }

        // Try ordinal-based match first (works in both modes).
        if let Some(entry) = self.entries.get(ordinal as usize) {
            if entry.name == name && entry.input_hash == input_hash {
                return match &entry.output {
                    Some(output) => ReplayResult::Hit(output.clone()),
                    None => ReplayResult::Miss,
                };
            }

            // Ordinal matched but name or hash differs.
            if self.mode == ReplayMode::Strict {
                if entry.name != name {
                    // Name mismatch at this ordinal — treat as divergence.
                    return ReplayResult::Mismatch {
                        expected: entry.input_hash,
                        actual: input_hash,
                    };
                }
                // Name matches but hash differs.
                return ReplayResult::Mismatch {
                    expected: entry.input_hash,
                    actual: input_hash,
                };
            }
        } else if self.mode == ReplayMode::Strict {
            return ReplayResult::Miss;
        }

        // Lenient mode: scan forward from ordinal for a matching name.
        // input_hash encodes name + ordinal (via hash_step_identity), so shifted steps
        // always differ. We match by name, then use content_hash (when both sides
        // provide one) to filter out false positives from name collisions.
        let start = (ordinal as usize).saturating_add(1);
        for entry in self.entries.iter().skip(start) {
            if entry.name == name {
                // If both sides have a content hash and they differ, skip this entry
                // and keep scanning — the name matches but the input is different.
                if let (Some(lookup), Some(cached)) = (content_hash, entry.content_hash) {
                    if lookup != cached {
                        continue;
                    }
                }
                return match &entry.output {
                    Some(output) => ReplayResult::Hit(output.clone()),
                    None => ReplayResult::Miss,
                };
            }
        }

        ReplayResult::Miss
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
            content_hash: None,
            output,
            error: None,
            attempt: 1,
            events: vec![],
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

    // -- ReplayMode tests --

    #[test]
    fn lenient_mode_returns_miss_on_hash_mismatch() {
        let mut cache = ReplayCache::with_mode(ReplayMode::Lenient);
        let snapshot = make_snapshot(vec![make_step("a", 42, Some(serde_json::json!("x")))]);
        cache.seed_from(&snapshot);

        // In strict mode this would be Mismatch; lenient returns Miss.
        assert!(matches!(cache.check(0, 99), ReplayResult::Miss));
    }

    #[test]
    fn strict_mode_mismatch_on_wrong_hash() {
        let mut cache = ReplayCache::with_mode(ReplayMode::Strict);
        let snapshot = make_snapshot(vec![make_step("a", 42, Some(serde_json::json!("x")))]);
        cache.seed_from(&snapshot);

        assert!(matches!(cache.check(0, 99), ReplayResult::Mismatch { .. }));
    }

    // -- check_by_name tests --

    #[test]
    fn by_name_hit_at_ordinal() {
        let mut cache = ReplayCache::with_mode(ReplayMode::Lenient);
        let snapshot = make_snapshot(vec![
            make_step("fetch", 10, Some(serde_json::json!("data"))),
            make_step("parse", 20, Some(serde_json::json!("parsed"))),
        ]);
        cache.seed_from(&snapshot);

        match cache.check_by_name("fetch", 0, 10, None) {
            ReplayResult::Hit(val) => assert_eq!(val, serde_json::json!("data")),
            other => panic!("expected Hit, got {other:?}"),
        }
    }

    #[test]
    fn by_name_scans_forward_in_lenient() {
        let mut cache = ReplayCache::with_mode(ReplayMode::Lenient);
        // Previous trace: fetch, transform, parse
        // New execution skips transform, so "parse" is at ordinal 1 but cached at ordinal 2.
        let snapshot = make_snapshot(vec![
            make_step("fetch", 10, Some(serde_json::json!("data"))),
            make_step("transform", 20, Some(serde_json::json!("t"))),
            make_step("parse", 30, Some(serde_json::json!("parsed"))),
        ]);
        cache.seed_from(&snapshot);

        // Asking for "parse" at ordinal 1 (where "transform" is cached).
        // Lenient mode scans forward by name and finds "parse" at index 2.
        // The hash differs (input_hash encodes ordinal), but lenient matches by name.
        match cache.check_by_name("parse", 1, 999, None) {
            ReplayResult::Hit(val) => assert_eq!(val, serde_json::json!("parsed")),
            other => panic!("expected Hit from forward scan, got {other:?}"),
        }
    }

    #[test]
    fn by_name_strict_rejects_name_mismatch() {
        let mut cache = ReplayCache::with_mode(ReplayMode::Strict);
        let snapshot = make_snapshot(vec![make_step(
            "fetch",
            10,
            Some(serde_json::json!("data")),
        )]);
        cache.seed_from(&snapshot);

        // Name "parse" at ordinal 0 where "fetch" is cached — strict rejects.
        assert!(matches!(
            cache.check_by_name("parse", 0, 10, None),
            ReplayResult::Mismatch { .. }
        ));
    }

    #[test]
    fn by_name_lenient_miss_when_name_not_found() {
        let mut cache = ReplayCache::with_mode(ReplayMode::Lenient);
        let snapshot = make_snapshot(vec![make_step(
            "fetch",
            10,
            Some(serde_json::json!("data")),
        )]);
        cache.seed_from(&snapshot);

        // Name "unknown" doesn't exist anywhere — Miss.
        assert!(matches!(
            cache.check_by_name("unknown", 0, 99, None),
            ReplayResult::Miss
        ));
    }

    #[test]
    fn by_name_lenient_miss_on_hash_mismatch() {
        let mut cache = ReplayCache::with_mode(ReplayMode::Lenient);
        let snapshot = make_snapshot(vec![make_step(
            "fetch",
            10,
            Some(serde_json::json!("data")),
        )]);
        cache.seed_from(&snapshot);

        // Right name, wrong hash — lenient returns Miss (re-execute).
        assert!(matches!(
            cache.check_by_name("fetch", 0, 99, None),
            ReplayResult::Miss
        ));
    }

    // -- content_hash identity tests --

    fn make_step_with_content(
        name: &str,
        input_hash: u64,
        content_hash: Option<u64>,
        output: Option<serde_json::Value>,
    ) -> Step {
        Step {
            name: name.into(),
            kind: StepKind::Plain,
            status: StepStatus::Ok,
            confidence: 1.0,
            started_at: Utc::now(),
            duration_ms: 0,
            input_hash,
            content_hash,
            output,
            error: None,
            attempt: 1,
            events: vec![],
        }
    }

    #[test]
    fn forward_scan_skips_content_hash_mismatch() {
        let mut cache = ReplayCache::with_mode(ReplayMode::Lenient);
        // Two steps named "fetch" with different content hashes.
        let snapshot = make_snapshot(vec![
            make_step("other", 1, Some(serde_json::json!("x"))),
            make_step_with_content("fetch", 10, Some(100), Some(serde_json::json!("old_input"))),
            make_step_with_content("fetch", 20, Some(200), Some(serde_json::json!("new_input"))),
        ]);
        cache.seed_from(&snapshot);

        // Looking for "fetch" at ordinal 0 with content_hash 200 — should skip the
        // entry at index 1 (content_hash 100) and hit index 2 (content_hash 200).
        match cache.check_by_name("fetch", 0, 999, Some(200)) {
            ReplayResult::Hit(val) => assert_eq!(val, serde_json::json!("new_input")),
            other => panic!("expected Hit with matching content_hash, got {other:?}"),
        }
    }

    #[test]
    fn forward_scan_hits_matching_content_hash() {
        let mut cache = ReplayCache::with_mode(ReplayMode::Lenient);
        let snapshot = make_snapshot(vec![
            make_step("other", 1, Some(serde_json::json!("x"))),
            make_step_with_content("fetch", 10, Some(100), Some(serde_json::json!("data"))),
        ]);
        cache.seed_from(&snapshot);

        // content_hash matches — should hit.
        match cache.check_by_name("fetch", 0, 999, Some(100)) {
            ReplayResult::Hit(val) => assert_eq!(val, serde_json::json!("data")),
            other => panic!("expected Hit, got {other:?}"),
        }
    }

    #[test]
    fn forward_scan_falls_back_to_name_when_no_content_hash() {
        let mut cache = ReplayCache::with_mode(ReplayMode::Lenient);
        // Cached entry has content_hash but lookup does not — should still hit by name.
        let snapshot = make_snapshot(vec![
            make_step("other", 1, Some(serde_json::json!("x"))),
            make_step_with_content("fetch", 10, Some(100), Some(serde_json::json!("data"))),
        ]);
        cache.seed_from(&snapshot);

        match cache.check_by_name("fetch", 0, 999, None) {
            ReplayResult::Hit(val) => assert_eq!(val, serde_json::json!("data")),
            other => panic!("expected Hit (no content_hash filter), got {other:?}"),
        }
    }

    #[test]
    fn forward_scan_falls_back_when_cached_has_no_content_hash() {
        let mut cache = ReplayCache::with_mode(ReplayMode::Lenient);
        // Cached entry has no content_hash, lookup does — should still hit by name.
        let snapshot = make_snapshot(vec![
            make_step("other", 1, Some(serde_json::json!("x"))),
            make_step("fetch", 10, Some(serde_json::json!("data"))),
        ]);
        cache.seed_from(&snapshot);

        match cache.check_by_name("fetch", 0, 999, Some(100)) {
            ReplayResult::Hit(val) => assert_eq!(val, serde_json::json!("data")),
            other => panic!("expected Hit (cached has no content_hash), got {other:?}"),
        }
    }

    #[test]
    fn forward_scan_miss_when_all_content_hashes_differ() {
        let mut cache = ReplayCache::with_mode(ReplayMode::Lenient);
        let snapshot = make_snapshot(vec![
            make_step("other", 1, Some(serde_json::json!("x"))),
            make_step_with_content("fetch", 10, Some(100), Some(serde_json::json!("a"))),
            make_step_with_content("fetch", 20, Some(200), Some(serde_json::json!("b"))),
        ]);
        cache.seed_from(&snapshot);

        // Looking for "fetch" at ordinal 0 (where "other" is cached). input_hash
        // differs, so falls through to forward scan. Scan finds "fetch" at index 1
        // (content_hash 100 != 999) — skip. Index 2 (content_hash 200 != 999) — skip.
        // No more entries — Miss.
        assert!(matches!(
            cache.check_by_name("fetch", 0, 999, Some(999)),
            ReplayResult::Miss
        ));
    }
}

#[cfg(test)]
mod proptest_replay {
    use super::*;
    use crate::types::step::{Step, StepKind, StepStatus};
    use chrono::Utc;
    use proptest::prelude::*;

    fn arb_step_name() -> impl Strategy<Value = String> {
        "[a-z]{1,8}"
    }

    fn arb_input_hash() -> impl Strategy<Value = u64> {
        any::<u64>()
    }

    fn make_step_prop(name: String, hash: u64, output: Option<serde_json::Value>) -> Step {
        Step {
            name,
            kind: StepKind::Plain,
            status: StepStatus::Ok,
            confidence: 1.0,
            started_at: Utc::now(),
            duration_ms: 0,
            input_hash: hash,
            content_hash: None,
            output,
            error: None,
            attempt: 1,
            events: vec![],
        }
    }

    fn make_snapshot_prop(steps: Vec<Step>) -> Crux<serde_json::Value> {
        use crate::types::id::CruxId;
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

    proptest! {
        /// Disabled cache always returns Miss, regardless of ordinal/hash.
        #[test]
        fn disabled_cache_always_misses(ordinal in 0u32..100, hash in arb_input_hash()) {
            let cache = ReplayCache::new();
            prop_assert!(!cache.is_enabled());
            prop_assert!(matches!(cache.check(ordinal, hash), ReplayResult::Miss));
        }

        /// After seeding, looking up ordinal 0 with the correct hash returns Hit.
        #[test]
        fn seeded_cache_hits_first_entry(name in arb_step_name(), hash in arb_input_hash()) {
            let mut cache = ReplayCache::new();
            let output = serde_json::json!("cached");
            let snapshot = make_snapshot_prop(vec![make_step_prop(
                name,
                hash,
                Some(output.clone()),
            )]);
            cache.seed_from(&snapshot);
            prop_assert!(cache.is_enabled());
            prop_assert!(matches!(cache.check(0, hash), ReplayResult::Hit(_)));
        }

        /// Out-of-bounds ordinal always returns Miss.
        #[test]
        fn out_of_bounds_ordinal_is_miss(
            name in arb_step_name(),
            hash in arb_input_hash(),
        ) {
            let mut cache = ReplayCache::new();
            let snapshot = make_snapshot_prop(vec![make_step_prop(
                name,
                hash,
                Some(serde_json::json!(1)),
            )]);
            cache.seed_from(&snapshot);
            // Ordinal 1 is past the end of a 1-entry cache.
            prop_assert!(matches!(cache.check(1, hash), ReplayResult::Miss));
        }

        /// In Strict mode, a hash mismatch at a valid ordinal returns Mismatch.
        #[test]
        fn strict_hash_mismatch_returns_mismatch(
            name in arb_step_name(),
            stored_hash in 1u64..u64::MAX,
        ) {
            let lookup_hash = stored_hash - 1; // guaranteed different
            let mut cache = ReplayCache::with_mode(ReplayMode::Strict);
            let snapshot = make_snapshot_prop(vec![make_step_prop(
                name,
                stored_hash,
                Some(serde_json::json!(1)),
            )]);
            cache.seed_from(&snapshot);
            match cache.check(0, lookup_hash) {
                ReplayResult::Mismatch { expected, actual } => {
                    prop_assert_eq!(expected, stored_hash);
                    prop_assert_eq!(actual, lookup_hash);
                }
                other => prop_assert!(false, "expected Mismatch, got {other:?}"),
            }
        }

        /// In Lenient mode, a hash mismatch at a valid ordinal returns Miss (not Mismatch).
        #[test]
        fn lenient_hash_mismatch_returns_miss(
            name in arb_step_name(),
            stored_hash in 1u64..u64::MAX,
        ) {
            let lookup_hash = stored_hash - 1;
            let mut cache = ReplayCache::with_mode(ReplayMode::Lenient);
            let snapshot = make_snapshot_prop(vec![make_step_prop(
                name,
                stored_hash,
                Some(serde_json::json!(1)),
            )]);
            cache.seed_from(&snapshot);
            prop_assert!(matches!(cache.check(0, lookup_hash), ReplayResult::Miss));
        }

        /// A step with no output returns Miss even on hash match.
        #[test]
        fn no_output_returns_miss_on_match(name in arb_step_name(), hash in arb_input_hash()) {
            let mut cache = ReplayCache::new();
            let snapshot = make_snapshot_prop(vec![make_step_prop(name, hash, None)]);
            cache.seed_from(&snapshot);
            prop_assert!(matches!(cache.check(0, hash), ReplayResult::Miss));
        }

        /// Seeding multiple steps, then checking each ordinal with the correct hash hits.
        #[test]
        fn multi_step_all_hit(
            names in prop::collection::vec(arb_step_name(), 1..8),
            hashes in prop::collection::vec(arb_input_hash(), 1..8),
        ) {
            // Use the shorter of the two vecs.
            let len = names.len().min(hashes.len());
            let names = &names[..len];
            let hashes = &hashes[..len];

            let steps: Vec<Step> = names
                .iter()
                .zip(hashes.iter())
                .map(|(n, &h)| make_step_prop(n.clone(), h, Some(serde_json::json!(h))))
                .collect();

            let mut cache = ReplayCache::new();
            let snapshot = make_snapshot_prop(steps);
            cache.seed_from(&snapshot);

            for (i, &hash) in hashes.iter().enumerate() {
                prop_assert!(
                    matches!(cache.check(i as u32, hash), ReplayResult::Hit(_)),
                    "expected Hit at ordinal {i}"
                );
            }
        }
    }
}
