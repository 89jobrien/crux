---
title: SafetyPolicy
tags: [trait, runtime, port, safety]
---
# SafetyPolicy
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/safety.rs`

Trait for validating [[HarnessDiff]] against safety constraints.
Methods: `validate(diff, base)` → Result<(), [[SafetyViolation]]>,
`requires_approval(diff)` → bool.
