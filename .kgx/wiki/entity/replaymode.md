---
title: ReplayMode
tags: [type, runtime, replay]
---
# ReplayMode
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/replay.rs`

Enum: Strict (default, fail on mismatch) or Lenient (forward name scan for
ordinal shifts). Used by [[ReplayCache]] and set via [[CruxCtx]].set_replay_mode()
or [[#[crux::agent]]] `replay = "lenient"` attribute.
