---
title: Runtime and Context
source_document: crux_runtime_crate
tags: [runtime, context, combinators]
---

# Runtime (crux-runtime)

## Context Trait (DIP abstraction)
[[Context]] provides step/hook/budget methods. [[CruxCtx]] is the production adapter.

### Step Methods
- `step(name, f)` -- basic named step
- `step_keyed(name, key, f)` -- content key for lenient replay
- `step_with_confidence(name, confidence, f)` -- explicit confidence
- `step_retryable(name, confidence, make_fut)` -- auto retry
- `step_stream(name, f)` -- streaming with events
- `try_step(name, f)` -- arbitrary error → CruxErr

### Combinators on CruxCtx
- `pipe(name, input, stages)` -- sequential chain, per-stage steps
- `join_all(name, arms)` -- fan-out via futures::join_all
- `route_on_confidence(name, confidence, routes)` -- validated range routing
- `delegate(name, input)` → [[DelegationBuilder]] -- sub-agent with budget/hooks
- `speculate(name, arms)` → [[SpeculationBuilder]] -- pick_best_by / first_ok

### Replay
[[ReplayCache]] with [[ReplayMode]] (Strict/Lenient). Steps matched by
`hash_step_identity(name, ordinal)`. Lenient does forward name scan.

### Hooks
[[HookRegistry]] dispatches: pre-step gates ([[HookVerdict]]),
confidence/failure/budget handlers returning [[Recovery<T>]].

### Recording
[[StepRecorder]] accumulates steps. [[Redactor]] trait scrubs output.

## Delegation
[[DelegationBuilder]] -- fluent: `with_budget()`, `on_low_confidence()`,
`on_step_failure()`, `run()`. Creates child [[CruxCtx]].

## Speculation
[[SpeculationBuilder]] -- `pick_best_by(scorer)` runs all arms, winner Ok,
losers Rejected. `first_ok()` returns first success.
