---
title: Context
tags: [trait, core, port]
---
# Context
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/context.rs`

DIP abstraction over [[CruxCtx]] for testability. All step/hook/budget
methods are defined here. [[CruxCtx]] is the production adapter.

## Step Methods
- `step(name, f)`, `step_keyed(name, key, f)`, `step_with_confidence(name, confidence, f)`
- `step_retryable(name, confidence, make_fut)`, `step_stream(name, f)`, `try_step(name, f)`

## Hook Methods
- `on_low_confidence(threshold, handler)`, `on_step_failure(handler)`, `on_budget_exceeded(handler)`

## Budget Methods
- `set_max_retries(n)`, `set_budget(budget)`, `consume_budget(amount)`
- `budget()`, `remaining_budget()`

## Query Methods
- `step_count()`, `snapshot_steps()`
