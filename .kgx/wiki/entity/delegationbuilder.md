---
title: DelegationBuilder
tags: [type, runtime, combinator]
---
# DelegationBuilder
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/delegation.rs`

Fluent builder for delegating to a sub-[[Agent]]. Created by `CruxCtx.delegate(name, input)`.

## Methods
- `with_budget(budget: [[Budget]])` -- per-call budget
- `on_low_confidence(threshold, handler)` -- confidence hook returning [[Recovery<T>]]
- `on_step_failure(handler)` -- failure hook
- `run()` -- execute delegation, creates child [[CruxCtx]]
