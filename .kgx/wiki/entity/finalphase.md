---
title: FinalPhase
tags: [type, core]
---
# FinalPhase
**Crate:** [[crux-types]] | **File:** `crates/crux-types/src/crux_value.rs`

Severity-ordered enum (Ord derived): Succeeded < Skipped < Aborted < Failed < Errored.
Used in [[StepRecord]] for per-step finalization.
