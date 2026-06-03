---
title: Redactor
tags: [trait, runtime, port]
---
# Redactor
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/recorder.rs`

Trait (Send + Sync) for scrubbing sensitive data. Methods: `redact_output(Value) → Value`,
`redact_error(&str) → String`. Used by [[StepRecorder]].
