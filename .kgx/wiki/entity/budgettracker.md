---
title: BudgetTracker
tags: [type, runtime]
---
# BudgetTracker
**Crate:** [[crux-types]] | **File:** `crates/crux-types/src/budget.rs`

Tracks consumption against a [[Budget]]. Fields: `budget`, `used`.
Methods: `new(budget)`, `remaining()` (saturating), `consume(amount)`, `is_exceeded()`, `budget()`.
[[CruxCtx]] delegates budget tracking here.
