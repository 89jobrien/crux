---
title: ReplayCache
tags: [type, runtime, replay]
---
# ReplayCache
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/replay.rs`

Stores cached step outputs from prior [[Crux<T>]] trace. Matched by
`hash_step_identity(name, ordinal)`. Modes: [[ReplayMode]] Strict (fail on
mismatch) or Lenient (forward name scan). Methods: `seed_from(previous)`,
`set_mode()`, `check()`, `check_by_name()`.
