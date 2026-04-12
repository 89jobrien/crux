# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.1.0] - 2026-04-12

Initial release.

### Added

- `Crux<T>` execution trace type -- inspectable, serializable, replayable
- `CruxCtx` runtime with `step()`, `delegate()`, `speculate()`, `pipe()`, `join_all()`,
  `route_on_confidence()`, `step_stream()`
- `#[crux::agent]` proc macro with `replay` and `registry` attribute wiring
- `Agent` trait with lifecycle hooks (`on_low_confidence`, `on_step_failure`, `on_budget_exceeded`)
- `Recovery<T>` enum: Retry, RetryWith, Substitute, Escalate, Propagate, Skip, Continue
- `Budget` constraints: tokens, calls, duration, cost, combined
- `DelegationBuilder` with per-call-site budget and hooks, child `CruxCtx`
- `SpeculationBuilder` with `pick_best_by` and `first_ok` strategies
- `ReplayCache` with strict and lenient modes, content-hash identity filtering
- `TaskRegistry<B>` with submit/get/update_status/checkpoint/pending lifecycle
- `InMemoryBackend` adapter
- `RedbBackend` adapter (behind `redb` feature, pure-Rust embedded KV)
- Tracing instrumentation (behind `tracing` feature)
- Checkpoint/resume: `CruxCtx::snapshot()`, `checkpoint_to()`, `resume_from()`
- SOLID decomposition: `HookRegistry`, `StepRecorder`, `ReplayCache`, `Context` trait
- 196 tests (203 with redb feature)
