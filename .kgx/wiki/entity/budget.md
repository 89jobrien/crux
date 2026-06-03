---
title: Budget
tags: [type, core]
---
# Budget
**Crate:** [[crux-types]] | **File:** `crates/crux-types/src/budget.rs`

Tagged enum (`#[serde(tag = "kind")]`) for execution constraints.

## Variants
- `Tokens { limit: u64 }`, `Calls { limit: u64 }`, `Duration { limit_ms: u64 }`
- `CostCents { limit: u64 }`, `Combined { budgets: Vec<Budget> }`

## Constructors
`tokens(n)`, `calls(n)`, `duration(d)`, `cost_cents(n)`, `combined(vec)`

## Methods
`kind()` → [[BudgetKind]], `limit()` → u64. Default: `Tokens { limit: u64::MAX }`.
Tracked at runtime by [[BudgetTracker]].
