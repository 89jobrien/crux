---
title: SpeculationBuilder
tags: [type, runtime, combinator]
---
# SpeculationBuilder
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/speculation.rs`

Fluent builder for running multiple arms and selecting a winner.
Created by `CruxCtx.speculate(name, arms)`.

## Methods
- `pick_best_by(scorer: Fn(&T) -> f32)` -- runs all arms, highest score wins,
  losers marked [[StepStatus]]::Rejected
- `first_ok()` -- returns first success
