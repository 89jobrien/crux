# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Crux is an agentic DSL for Rust -- macros, traits, and types that make agentic control flow
explicit in the type system. Every step, delegation, speculation, and failure is a first-class
value (`Crux<T>`) that is inspectable, serializable, and replayable. Rust edition 2024, MSRV 1.88.

## Build Commands

```bash
just ci                              # Full gate: fmt + clippy + nextest
just test                            # cargo nextest run
just lint                            # cargo clippy --all-targets -- -D warnings
just fmt                             # cargo fmt --all -- --check
just fix                             # cargo fmt --all (in-place)
just build                           # cargo build --all-targets
just hooks                           # Install git hooks from .githooks/

cargo nextest run -p cruxx-core       # Test a single crate
cargo nextest run test_name          # Run a single test
cargo nextest run --features redb    # Include redb adapter tests
```

Always use `cargo nextest run` instead of `cargo test`.

## Workspace Structure

Crates in `crates/`:

- **`cruxx`** -- Facade crate. Re-exports `cruxx-core` + `cruxx-macros`. Integration tests live here
  (`tests/agent_macro.rs`, `combinators.rs`, `delegation.rs`, `speculation.rs`, `task_registry.rs`).
- **`cruxx-core`** -- All domain logic: types, traits, runtime. Includes orchestrator types
  (`HarnessProfile`, `ResourceHints`, `HarnessDiff`, `EvolutionOutcome`) and ports
  (`SafetyPolicy`, `ApprovalGate`).
- **`cruxx-macros`** -- `#[cruxx::agent]`, `#[cruxx::harness]`, `#[cruxx::evolve]` proc macros.
- **`cruxx-agentic`** -- Step handlers: shell, fs, git, json, llm, container, harness. Adapters:
  `AutoApproveGate`, `TerminalApprovalGate`.
- **`cruxx-planner`** -- `EvolutionPlanner`: deterministic, metrics-driven harness profile evolution.
  Accepts `RunMetrics`, emits `HarnessDiff`.
- **`cruxx-script`** -- YAML-driven pipeline scripting.
- **`cruxx-types`** -- Wire-format types (`Crux<T>`, `Step`, `Budget`, `CruxId`, `CruxErr`) with
  minimal deps (serde, chrono, ulid). `cruxx-core` re-exports everything — no breaking change.
  External consumers (minibox) depend on this to avoid pulling the full runtime. `RecoveryKind`
  is the serializable subset of `Recovery<T>` (closure variants stay in core).
- **`cruxx-model`** -- Canonical model ID types and provider-specific parsers.
- **`cruxx-plugin`** -- Subprocess plugin host for pipelines.

## Feature Flags

| Flag | Default | Effect |
|------|---------|--------|
| `tokio-runtime` | yes | Enables async support (tokio + futures). Required for compilation. |
| `redb` | no | `RedbBackend` via redb (pure-Rust embedded KV store). |
| `tracing` | no | Instrument with tracing spans. |

## Architecture

### Hexagonal / Ports-and-Adapters

The `RegistryBackend` trait is the persistence port. Two adapters exist:
`InMemoryBackend` (default) and `RedbBackend` (behind `redb` feature).

`Context` trait (`context.rs`) is the DIP abstraction over `CruxCtx` for testability.
`Agent::run` takes `&mut CruxCtx` directly -- use the `Context` trait boundary to inject mocks.

### SOLID Decomposition

`CruxCtx` delegates to collaborators, each independently testable:

- `HookRegistry` (`hooks.rs`) -- lifecycle hook dispatch
- `StepRecorder` (`recorder.rs`) -- appends steps to the trace
- `ReplayCache` (`replay.rs`) -- step output cache with strict/lenient modes

### Key Types

- `Crux<T>` (`types/cruxx_value.rs`) -- execution trace fused with result
- `Step` (`types/step.rs`) -- recorded unit of work (kind, status, confidence, output, children)
- `CruxCtx` (`ctx.rs`) -- runtime: `step()`, `delegate()`, `speculate()`, `pipe()`, `join_all()`,
  `route_on_confidence()`
- `Agent` trait (`agent.rs`) -- `name()`, `run(ctx, input)`, `budget()`, lifecycle hooks
- `TaskRegistry<B>` (`registry/mod.rs`) -- submit/get/update_status/checkpoint/pending with CAS
- `Recovery<T>` (`types/recovery.rs`) -- hook return: Continue, Skip, Retry, Escalate, Substitute(T)
- `Budget` (`types/budget.rs`) -- token/step/time limits, scoped per delegation
- `HarnessProfile` (`types/harness.rs`) -- resource spec for a container/process harness
- `ResourceHints` -- advisory scheduling metadata attached to a profile
- `HarnessDiff` -- incremental description of profile changes
- `EvolutionOutcome` -- result of applying a diff (accepted, rejected, or pending approval)
- `SafetyPolicy` trait -- port for diff approval logic; returns Approved/Rejected/RequiresApproval
- `ApprovalGate` trait -- hook-level port called when `SafetyPolicy` returns `RequiresApproval`

### Replay

Steps are matched by name + ordinal hash (`hash_step_identity`). Strict mode fails on mismatch.
Lenient mode does a forward name scan, so ordinal shifts are expected -- the scan is the designed
recovery path, not a fallback.

### Proc Macros

`#[cruxx::agent]` on `async fn foo(input: T) -> Crux<U>` generates:
1. Inner function with `CruxCtx` injected as `t`
2. Public wrapper that creates `CruxCtx` and calls `finalize()`
3. `FooAgent` struct implementing the `Agent` trait

`#[cruxx::harness]` on a struct marks it as a managed container/process harness. The struct
must have `image: String` and any additional fields mapped to `HarnessProfile`.

`#[cruxx::evolve]` on `async fn f(metrics: RunMetrics) -> Crux<EvolutionOutcome>` injects
an `EvolutionPlanner` (as `planner`) and a `CruxCtx` (as `x`) into the function body.

## Pipeline Files

Pipeline definitions use the `.crux` file extension (YAML syntax). Previously `.yaml` and `.cruxx`.

## BAML (cruxx-agentic)

- `just check-baml` — validates `generators.baml` version matches `Cargo.toml` baml dep;
  auto-downloads native lib if missing. Run after any baml version bump.
- `baml_client/` is gitignored (generated). Run `mise exec -- baml-cli generate` after cloning
  or bumping the baml version. The `baml` crate version in `Cargo.toml` must match `version` in
  `generators.baml` exactly. When bumping baml, update both files together.
- `baml-cli` is managed via `.mise.toml` — always use `mise exec -- baml-cli generate` from
  `crates/cruxx-agentic/`. Never run bare `baml-cli generate`; the global shim may be stale.
- Build `cruxx-run` with `--features baml` or `llm::extract` / `llm::decompose` won't register.
- Run pipeline examples: `dotenvx run --env-file=$HOME/dev/.env -- ./target/debug/cruxx-run
  examples/<pipeline>.cruxx examples/input_<name>.json`
- BAML integration tests and examples require API keys from `~/dev/.env` — see `CLAUDE.local.md`
  for the exact injection commands (machine-local, gitignored).

### Combinators on CruxCtx

- `pipe()` -- chains sequential closures, records per-stage steps
- `join_all()` -- fans out via `futures::join_all`, records per-arm steps
- `route_on_confidence()` -- validates non-overlapping, gap-free, [0.0,1.0]-covering ranges
- `DelegationBuilder` -- fluent API with per-call-site budget/hooks, child CruxCtx
- `SpeculationBuilder` -- `pick_best_by`, `first_ok`, racing; losers marked `Rejected`

## Pipeline Capabilities Reference

See `docs/crux-capabilities.md` for the full list of supported step types, handlers, and known gaps.
