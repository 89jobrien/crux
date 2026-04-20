# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Crux is an agentic DSL for Rust -- macros, traits, and types that make agentic control flow
explicit in the type system. Every step, delegation, speculation, and failure is a first-class
value (`Crux<T>`) that is inspectable, serializable, and replayable. Rust edition 2024, MSRV 1.85.

## Build Commands

```bash
just ci                              # Full gate: fmt + clippy + nextest
just test                            # cargo nextest run
just lint                            # cargo clippy --all-targets -- -D warnings
just fmt                             # cargo fmt --all -- --check
just fix                             # cargo fmt --all (in-place)
just build                           # cargo build --all-targets
just hooks                           # Install git hooks from .githooks/

cargo nextest run -p crux-core       # Test a single crate
cargo nextest run test_name          # Run a single test
cargo nextest run --features redb    # Include redb adapter tests
```

Always use `cargo nextest run` instead of `cargo test`.

## Workspace Structure

Three crates in `crates/`:

- **`crux`** -- Facade crate. Re-exports `crux-core` + `crux-macros`. Integration tests live here
  (`tests/agent_macro.rs`, `combinators.rs`, `delegation.rs`, `speculation.rs`, `task_registry.rs`).
- **`crux-core`** -- All domain logic: types, traits, runtime. This is where most code changes happen.
- **`crux-macros`** -- `#[crux::agent]` proc macro. Transforms an `async fn` into an `Agent` trait
  impl + wrapper struct (e.g. `hello` -> `HelloAgent`).

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

- `Crux<T>` (`types/crux_value.rs`) -- execution trace fused with result
- `Step` (`types/step.rs`) -- recorded unit of work (kind, status, confidence, output, children)
- `CruxCtx` (`ctx.rs`) -- runtime: `step()`, `delegate()`, `speculate()`, `pipe()`, `join_all()`,
  `route_on_confidence()`
- `Agent` trait (`agent.rs`) -- `name()`, `run(ctx, input)`, `budget()`, lifecycle hooks
- `TaskRegistry<B>` (`registry/mod.rs`) -- submit/get/update_status/checkpoint/pending with CAS
- `Recovery<T>` (`types/recovery.rs`) -- hook return: Continue, Skip, Retry, Escalate, Substitute(T)
- `Budget` (`types/budget.rs`) -- token/step/time limits, scoped per delegation

### Replay

Steps are matched by name + ordinal hash (`hash_step_identity`). Strict mode fails on mismatch.
Lenient mode does a forward name scan, so ordinal shifts are expected -- the scan is the designed
recovery path, not a fallback.

### Proc Macro

`#[crux::agent]` on `async fn foo(input: T) -> Crux<U>` generates:
1. Inner function with `CruxCtx` injected as `t`
2. Public wrapper that creates `CruxCtx` and calls `finalize()`
3. `FooAgent` struct implementing the `Agent` trait

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
