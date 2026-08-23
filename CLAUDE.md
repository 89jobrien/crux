# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working
with code in this repository.

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

cargo nextest run -p crux-runtime       # Test a single crate
cargo nextest run test_name          # Run a single test
cargo nextest run --features redb    # Include redb adapter tests
```

Always use `cargo nextest run` instead of `cargo test`.

## Workspace Structure

Crates in `crates/`:

- **`crux`** -- Facade crate. Re-exports `crux-domain`, `crux-derive`, `crux-runtime` (optional
  `crux-script`). Integration tests live here (`tests/agent_macro.rs`, `checkpoint.rs`,
  `combinators.rs`, `conformance.rs`, `delegation.rs`, `evolve_macro.rs`, `harness_macro.rs`,
  `research_pipeline.rs`, `speculation.rs`, `streaming.rs`, `substrate_integration.rs`,
  `task_registry.rs`).
- **`crux-runtime`** -- All domain logic: types, traits, runtime. Includes orchestrator types
  (`HarnessProfile`, `ResourceHints`, `HarnessDiff`, `EvolutionOutcome`) and ports
  (`SafetyPolicy`, `ApprovalGate`).
- **`crux-macros`** (package name `crux-derive`, renamed for crates.io publishing) --
  `#[crux::agent]`, `#[crux::harness]`, `#[crux::evolve]` proc macros.
- **`crux-agentic`** -- Step handlers: shell, fs, git, json, llm, container, harness. Adapters:
  `AutoApproveGate`, `TerminalApprovalGate`.
- **`crux-cli`** -- `crux` CLI binary (`run`/`plan`/`check` subcommands), depends on `crux-agentic`
  for handlers/registry.
- **`crux-planner`** -- `EvolutionPlanner`: deterministic, metrics-driven
  harness profile evolution. Accepts `RunMetrics`, emits `HarnessDiff`.
- **`crux-script`** -- YAML-driven pipeline scripting.
- **`crux-types`** -- Wire-format types (`Crux<T>`, `Step`, `Budget`, `CruxId`, `CruxErr`) with
  minimal deps (serde, chrono, ulid). `crux-runtime` re-exports everything — no breaking change.
  External consumers (minibox) depend on this to avoid pulling the full runtime. `RecoveryKind`
  is the serializable subset of `Recovery<T>` (closure variants stay in core).
- **`crux-model`** -- Canonical model ID types and provider-specific parsers.
- **`crux-plugin`** -- Subprocess plugin host for pipelines.
- **`crux-domain`** -- Pure domain types for the crux agentic DSL -- no async, no LLM deps.
- **`crux-baml`** -- BAML-powered LLM handlers for crux-script pipelines (extract, decompose, plan).
- **`crux-stdlib`** -- Standard library handlers for crux-script pipelines (fs, git, json, text, ctrl).
- **`crux-task`** -- Project task management for crux.
- **`crux-improve`** -- Improvement protocol for the crux agent runtime: strategies, diffs,
  comparisons, and policies, built on `crux-types` trace types.

## Feature Flags

| Flag            | Default | Effect                                                             |
| --------------- | ------- | ------------------------------------------------------------------ |
| `tokio-runtime` | yes     | Enables async support (tokio + futures). Required for compilation. |
| `redb`          | no      | `RedbBackend` via redb (pure-Rust embedded KV store).              |
| `tracing`       | no      | Instrument with tracing spans.                                     |

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

- `Crux<T>` (`types/crux_value.rs`) -- execution trace fused with result
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

`#[crux::agent]` on `async fn foo(input: T) -> Crux<U>` generates:

1. Inner function with `CruxCtx` injected as `t`
2. Public wrapper that creates `CruxCtx` and calls `finalize()`
3. `FooAgent` struct implementing the `Agent` trait

`#[crux::harness]` on a struct marks it as a managed container/process harness. The struct
must have `image: String` and any additional fields mapped to `HarnessProfile`.

`#[crux::evolve]` on `async fn f(metrics: RunMetrics) -> Crux<EvolutionOutcome>` injects
an `EvolutionPlanner` (as `planner`) and a `CruxCtx` (as `x`) into the function body.

## Pipeline Files

Pipeline definitions use the `.crux` file extension (YAML syntax). Previously `.yaml` and `.crux`.

## BAML (crux-agentic)

- `just check-baml` — validates `generators.baml` version matches `Cargo.toml` baml dep;
  auto-downloads native lib if missing. Run after any baml version bump.
- `baml_client/` is gitignored (generated). Run `mise exec -- baml-cli generate` after cloning
  or bumping the baml version. The `baml` crate version in `Cargo.toml` must match `version` in
  `generators.baml` exactly. When bumping baml, update both files together.
- `baml-cli` is managed via `.mise.toml` — always use `mise exec -- baml-cli generate` from
  `crates/crux-agentic/`. Never run bare `baml-cli generate`; the global shim may be stale.
- Build `crux-run` with `--features baml` or `llm::extract` / `llm::decompose` won't register.
- Run pipeline examples: `dotenvx run --env-file=$HOME/dev/.env -- ./target/debug/crux-run
examples/<pipeline>.crux examples/input_<name>.json`
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
