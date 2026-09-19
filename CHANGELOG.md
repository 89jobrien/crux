<!-- markdownlint-disable MD024 -->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.4.2] - 2026-09-19

### Fixed

- Include the generated Rust BAML client in source archives so `crux-baml`
  can be independently packaged and compiled.

## [0.4.1] - 2026-09-19

### Changed

- Repositioned the README around `Crux<T>` as a typed execution value and
  updated installation examples for the patch release.

## [0.4.0] - 2026-09-12

### Added

- Dedicated `crux-cli` package for the `crux` binary, including optional BAML planner wiring.
- Pipeline controls for retry/backoff, `timeout_ms`, `allow_failure`, `expect`, `on_error`,
  `vars`, `poll`, `for_each`, `while`, and `repeat`.
- Typed step, token, duration, and USD metering through `UsdAmount`, `HandlerUsage`,
  `BudgetUsage`, and `HandlerExecution`, which preserves usage for successful and failed calls.
- `json::jq` support for pipelines, `select(...)`, `map(...)`, comparisons, and indexed paths.
- Wiring and integration coverage for all nine supported BAML extraction functions.

### Changed

- `crux run` now prints a concise human-readable summary by default; `--verbose` prints the
  full trace and explicit `--json` emits only the compact machine-readable result.
- Pipeline budgets now reserve each real handler attempt and account reported usage exactly and
  monotonically. Canonical fields are `steps`, `tokens`, `duration_ms`, and `usd`; `calls` and
  `cost_cents` remain compatibility spellings.
- Consolidated shared planning types in `crux-types`, split triage handlers into focused modules,
  and reorganized runtime orchestration modules to remove dependency cycles.
- Hardened CI and releases with MSRV 1.89.0 checks, nextest/machete/deny gates, reliable BAML
  stubs and protoc setup, and dependency-ordered `cargo xtask publish` with index polling.

### Fixed

- Budget enforcement now keeps combined dimensions independent, charges failed paid handlers,
  skips replay hits, and fails closed when a USD-budgeted handler does not report cost.
- Pipeline expressions no longer strip repeated `output` path segments, and confidence values
  are validated instead of silently accepting out-of-range values.
- Human and JSON output modes preserve their respective contracts, including readable shell
  output and structured JSON errors.
- Updated examples and documentation to use real handlers, current package names, and the
  expanded pipeline control-flow surface.

### Breaking Changes

- The `crux` binary moved from `crux-agentic` to `crux-cli`; install or build `crux-cli` when the
  command-line application is required.
- The standalone `crux check` subcommand was removed; use `crux run FILE --check`.
- Default `crux run` output is no longer raw JSON. Automation must pass `--json` explicitly;
  that mode is not supported for multi-target Cruxfile runs.
- The proc-macro package is now published as `crux-derive` instead of `crux-macros`; direct
  dependencies must use the new package name.
- `BoxHandler` now resolves to `HandlerExecution` rather than
  `Result<HandlerOutput, CruxErr>`; direct handler callers must inspect `.outcome`, and custom
  metered handlers should use the new metered or explicitly-free registration APIs.
- Missing handler confidence now defaults to `0.5` instead of `1.0`. `Budget`, `BudgetKind`, and
  `CruxErr` also have new typed metering variants that exhaustive matches must handle.
- Removed unused public helpers including `load_cruxfile_file`, `OllamaAdapter::list_models`,
  `TomlFileDiscovery::default_path`, and `register_echo_agent`.

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

[Unreleased]: https://github.com/89jobrien/crux/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/89jobrien/crux/compare/v0.3.1...v0.4.0
