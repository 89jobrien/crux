---
title: CruxCtx
tags: [type, runtime, context]
---
# CruxCtx
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/ctx.rs`

Production runtime adapter. Implements [[Context]] trait. Delegates to
[[StepRecorder]], [[HookRegistry]], [[ReplayCache]], [[BudgetTracker]].

## Construction
- `new(agent_name)` -- create fresh context
- `set_planner(impl Planner)` -- attach planner
- `set_event_sender(sender)` -- attach event pipeline
- `set_redactor(redactor)` -- attach output scrubber

## Combinators
- `pipe(name, input, stages)` -- sequential chain
- `join_all(name, arms)` -- parallel fan-out
- `route_on_confidence(name, confidence, routes)` -- validated range routing
- `delegate(name, input)` → [[DelegationBuilder]]
- `speculate(name, arms)` → [[SpeculationBuilder]]

## Replay & Checkpoint
- `replay_from(previous)` -- seed replay cache
- `set_replay_mode(mode)` -- Strict/Lenient
- `snapshot()` -- type-erased checkpoint
- `checkpoint_to(registry, task_id)` -- persist to [[TaskRegistry]]
- `resume_from(registry, task_id)` -- restore from registry

## Finalization
- `finalize(result)` → [[Crux<T>]] -- produces final trace
