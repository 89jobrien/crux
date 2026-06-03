---
title: CruxErr
tags: [type, core, error]
---
# CruxErr
**Crate:** [[crux-types]] | **File:** `crates/crux-types/src/error.rs`

Tagged enum (`#[serde(tag = "kind")]`). Variants: StepFailed, LowConfidence,
BudgetExceeded (refs [[BudgetKind]]), Delegation (recursive Box<CruxErr>),
Cancelled, ReplayMismatch, Denied.

Methods: `step_failed()`, `low_confidence()`, `failed_step()` (recursive),
`is_transient()` (retryable check). Implements Error + Display.
