# crux-derive

Procedural macros for Crux's typed Rust surface. The package is named `crux-derive` and is normally
consumed through the `crux` facade as `#[crux::agent]`, `#[crux::harness]`, and `#[crux::evolve]`.

## Architecture role

This proc-macro crate parses user syntax and emits code that targets stable `crux_runtime` paths. It
cannot depend on runtime types directly across the proc-macro boundary. Generated behavior is tested
from `crates/crux/tests/`, not by treating expansion internals as a public API.

## Macros and generated API

### `#[crux::agent]`

Apply to an async function returning `Crux<T>`. The expansion injects a mutable `CruxCtx` binding
named `x`, creates the public wrapper that finalizes the trace, and generates `<FunctionName>Agent`
implementing `Agent`.

```rust
use crux::prelude::*;

#[crux::agent(replay = "lenient")]
async fn greet(name: String) -> Crux<String> {
    x.step("format", || async move { Ok(format!("Hello, {name}")) }).await
}
```

Supported options are `registry = "binding"`, `checkpoint_every_step`, and
`replay = "strict" | "lenient"`. Unknown options and invalid replay modes are compile errors.

### `#[crux::harness]`

Accepts a named-field struct and generates `Debug`, `Clone`, serde traits, `Default`, and
`to_profile(id)`. The current expansion expects the fields `memory_mb`, `cpu_millicores`,
`timeout_seconds`, and `network_access`, with defaults 512, 1000, 300, and `false`.

### `#[crux::evolve]`

Uses agent-style expansion and adds `is_evolution_agent() -> true` to the generated agent type.

## Features and status

There are no Cargo features. Expansion currently relies on the downstream crate having
`crux_runtime` available; harness expansion also refers to `serde`. The generated API is source code
at compile time and must not be hand-maintained elsewhere.

## Development and testing

```console
cargo nextest run -p crux
cargo clippy -p crux-derive --all-targets -- -D warnings
cargo expand -p crux --test agent_macro
```

Integration coverage includes zero-, one-, and multi-argument agents, failures, steps, confidence,
hooks, serialization, checkpointing, harness profiles, and evolution agents.
