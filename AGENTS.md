# AGENTS.md

Guidance for AI agents working with this codebase.

## Overview

Crux is an agentic DSL for Rust. It has two surfaces: YAML pipelines
(`.crux` files) for declarative workflows, and a Rust macro API
(`#[crux::agent]`) for typed control flow. Both produce a `Crux<T>`
trace that is inspectable, serializable, and replayable. Rust edition
2024, MSRV 1.88.

## Build and Test

```bash
just ci                           # Full gate: fmt + clippy + nextest
just test                         # cargo nextest run
just lint                         # cargo clippy --all-targets -- -D warnings
just fmt                          # cargo fmt --all -- --check
just fix                          # cargo fmt --all (in-place)
just build                        # cargo build --all-targets
```

Always use `cargo nextest run` instead of `cargo test`.

```bash
cargo nextest run -p crux-runtime    # Single crate
cargo nextest run test_name          # Single test
cargo nextest run --features redb    # Include redb adapter tests
```

## Workspace Structure

All crates live in `crates/`:

| Crate          | Role                                                                      |
| -------------- | ------------------------------------------------------------------------- |
| `crux`         | Facade -- re-exports runtime + macros. Integration tests here.            |
| `crux-runtime` | Core types, traits, runtime. All domain logic.                            |
| `crux-types`   | Wire-format types (`Crux<T>`, `Step`, `Budget`, `CruxErr`). Minimal deps. |
| `crux-macros`  | `#[crux::agent]`, `#[crux::harness]`, `#[crux::evolve]` proc macros.      |
| `crux-agentic` | Step handlers: shell, fs, git, json, llm, container, harness.             |
| `crux-script`  | YAML pipeline parsing and execution.                                      |
| `crux-model`   | Model ID types and provider parsers.                                      |
| `crux-plugin`  | Subprocess plugin host.                                                   |
| `crux-planner` | Metrics-driven harness profile evolution.                                 |

## Feature Flags

| Flag            | Default | Effect                                     |
| --------------- | ------- | ------------------------------------------ |
| `tokio-runtime` | yes     | Async support (tokio + futures). Required. |
| `redb`          | no      | `RedbBackend` persistent KV store.         |
| `tracing`       | no      | Instrument with tracing spans.             |
| `baml`          | no      | BAML-backed LLM extraction handlers.       |

## Architecture

### Hexagonal / Ports-and-Adapters

- `RegistryBackend` trait is the persistence port. Adapters:
  `InMemoryBackend` (default), `RedbBackend` (behind `redb` feature).
- `Context` trait (`context.rs`) abstracts `CruxCtx` for testability.

### Key Types

- `Crux<T>` -- execution trace fused with result
- `Step` -- recorded unit of work (kind, status, confidence, output)
- `CruxCtx` -- runtime context: `step()`, `delegate()`, `speculate()`,
  `pipe()`, `join_all()`, `route_on_confidence()`
- `Agent` trait -- `name()`, `run(ctx, input)`, `budget()`, hooks
- `TaskRegistry<B>` -- typed task management with CAS
- `Budget` -- token/step/time/cost limits
- `Recovery<T>` -- hook return: Continue, Skip, Retry, Escalate,
  Substitute(T)

### Proc Macros

`#[crux::agent]` on `async fn foo(input: T) -> Crux<U>` generates:

1. Inner function with `CruxCtx` injected as `x`
2. Public wrapper that creates `CruxCtx` and calls `finalize()`
3. `FooAgent` struct implementing the `Agent` trait

### Replay

Steps matched by name + ordinal hash. Strict mode fails on mismatch.
Lenient mode does a forward name scan for recovery.

## Pipeline Binary

The CLI binary is `crux` (not `crux-run`), built from `crux-agentic`:

```bash
cargo build -p crux-agentic --bin crux --release
crux run examples/showcase.crux
crux run examples/showcase.crux -q    # quiet
crux run examples/showcase.crux -v    # verbose
```

Pipeline files use `.crux` extension (YAML syntax).

## BAML

- `baml_client/` is generated and gitignored. Regenerate with
  `mise exec -- baml-cli generate` from `crates/crux-agentic/`.
- The `baml` crate version in `Cargo.toml` must match `version` in
  `generators.baml`. Update both together.
- BAML tests require API keys -- see `CLAUDE.local.md`.

## Conventions

- Run `cargo clippy` and `cargo nextest run` after changes.
- Run tests before committing.
- Use `cargo nextest run` for test filtering, not `cargo test`.
- Pipeline definitions use `.crux` extension.
- Do not modify generated files (`baml_client/`, `target/`).

## Documentation

- `docs/pipelines/` -- YAML pipeline walkthrough (6 chapters)
- `docs/walkthrough/` -- Rust API tutorial (7 chapters)
- `docs/crux-capabilities.md` -- handler reference and support matrix
- `docs/crux-syntax-reference.md` -- full syntax card
- `docs/crux-plugins.md` -- plugin system
- `book.toml` -- mdbook config; `mdbook serve` for local preview
