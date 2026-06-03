---
title: TrustScore
tags: [type, runtime, trust]
---
# TrustScore
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/trust.rs`

Per-agent trust with temporal decay. Fields: score(f64), successes, failures, last_updated.
Default: score=0.5. Methods: `record_success(reward)`, `record_failure(penalty)`,
`current(decay_rate)`, `reliability()` (success/total). Stored in [[TrustRegistry]].
