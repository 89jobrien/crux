# crux

An agentic DSL for Rust -- inspectable, serializable, replayable agent execution.

`crux` is not a standalone language. It's a set of macros, traits, and types that make agentic
control flow explicit in the Rust type system. If you've written agents with `tokio` + `tracing`
\+ a hand-rolled task queue, `crux` is what happens when you bake those patterns into the language
itself.

## Quick example

```rust
use crux::prelude::*;

#[crux::agent]
async fn plan_trip(goal: String) -> Crux<Itinerary> {
    let research = t.step("research", || search_web(&goal)).await?;

    let draft = t.delegate::<DraftAgent>("draft", &research)
        .with_budget(Budget::tokens(4000))
        .on_low_confidence(0.7, escalate_to_human)
        .await?;

    t.speculate("finalize", [
        ("cheap", || finalize_cheap(&draft)),
        ("fast",  || finalize_fast(&draft)),
        ("safe",  || finalize_safe(&draft)),
    ]).pick_best_by(|r| r.confidence).await
}
```

Every `t.step`, `t.delegate`, `t.speculate` call is recorded in the `Crux<T>` value
the function returns. That value is:

- **Inspectable** -- `crux.causal_chain()`, `crux.delegations()`, `crux.rejected_branches()`
- **Serializable** -- `serde_json::to_string(&crux)` just works
- **Replayable** -- `Crux::replay_from(snapshot)` resumes after a crash
- **Composable** -- `crux_a | crux_b`, `Crux::join_all([...])`

## Crates

| Crate | Description |
|-------|-------------|
| [`crux`](crates/crux) | Facade crate -- re-exports `crux-core` + `crux-macros` |
| [`crux-core`](crates/crux-core) | Core types, traits, and runtime |
| [`crux-macros`](crates/crux-macros) | `#[crux::agent]` proc macro |

## Features

Enable via `crux`:

| Feature | Default | Description |
|---------|---------|-------------|
| `tokio-runtime` | yes | Async runtime support via tokio + futures |
| `redb` | no | Persistent `TaskRegistry` backend via redb (pure-Rust) |
| `tracing` | no | Instrument with `tracing` spans |

## Core concepts

**`Crux<T>`** -- the execution trace. Every step, delegation, speculation, and failure is a
first-class value you can inspect, serialize, and replay.

**`CruxCtx`** -- the runtime context threaded through agent execution. Provides `step()`,
`delegate()`, `speculate()`, `pipe()`, `join_all()`, `route_on_confidence()`.

**`Agent` trait** -- the single-method interface all agents implement. The `#[crux::agent]` macro
generates this for you.

**`TaskRegistry<B>`** -- typed task management with submit, checkpoint, replay, and status
transitions. Pluggable backend (`InMemoryBackend`, `RedbBackend`).

**Lifecycle hooks** -- `on_low_confidence`, `on_step_failure`, `on_budget_exceeded` with recovery
actions (skip, retry, escalate, substitute).

**Replay** -- strict or lenient mode. Strict rejects hash mismatches; lenient skips removed steps
and returns cache misses for changed ones.

## Installation

```toml
[dependencies]
crux = "0.1"

# With persistent storage (redb, pure-Rust):
# crux = { version = "0.1", features = ["redb"] }
```

Requires Rust 1.85+ (edition 2024).

## Examples

```bash
cargo run --example basic_agent
```

See [`examples/`](examples/) for more.

## Documentation

See the [tutorial](docs/README.md) for a chapter-by-chapter walkthrough.

## License

MIT -- see [LICENSE](LICENSE).
