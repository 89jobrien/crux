---
title: StepRecorder
tags: [type, runtime]
---
# StepRecorder
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/recorder.rs`

Single responsibility: constructs and accumulates [[Step]] records.
Tracks ordinals. Optional [[Redactor]] scrubs output before recording.
Methods: `next_ordinal()`, `record_ok()`, `record_err()`, `record_skipped()`,
`record_replay()`, `push_raw()`. Public functions: `hash_step_identity()`,
`hash_content()`.
