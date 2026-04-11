# 01 — Setup & Rust toolchain

> Goal: get a `trace::` project building, run the smallest possible traced
> agent, and understand the shape of what comes out.

## Rust toolchain

`trace::` is a Rust DSL, so the toolchain is just Rust. You need:

- `rustc` 1.75+ (for native `async fn` in traits — `trace::` leans on this)
- `cargo`
- A stable runtime — we'll use `tokio` in every example

```bash
rustup toolchain install stable
rustup default stable
rustc --version   # 1.75 or newer
```

No separate compiler, no custom build tool. If Rust builds, `trace::` builds.

## Scaffolding a project

```bash
cargo new trip-planner
cd trip-planner
```

Edit `Cargo.toml`:

```toml
[package]
name = "trip-planner"
version = "0.1.0"
edition = "2021"

[dependencies]
trace = { version = "0.1", features = ["serde", "tokio"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Three feature flags matter on `trace`:

| Feature | What it turns on |
|---------|------------------|
| `serde` | `Serialize`/`Deserialize` on `Trace<T>`, `TaskRegistry`, `Step` |
| `tokio` | `t.delegate` uses `tokio::spawn`, `Trace::join_all` uses `tokio::join!` |
| `sqlite` | `TaskRegistry` can persist to SQLite (chapter 04) |

If you omit `tokio`, `trace::` falls back to a synchronous executor — useful
for tests but not for real agents.

## Your first traced function

Create `src/main.rs`:

```rust
use trace::prelude::*;

#[trace::agent]
async fn hello(name: String) -> Trace<String> {
    let greeting = t.step("greet", || async {
        Ok(format!("hello, {}", name))
    }).await?;

    let shouted = t.step("shout", || async {
        Ok(greeting.to_uppercase())
    }).await?;

    Ok(shouted)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let trace = hello("world".into()).await;

    println!("result: {:?}", trace.value());
    println!("steps:  {}", trace.causal_chain().len());
    println!("json:   {}", serde_json::to_string_pretty(&trace)?);
    Ok(())
}
```

Run it:

```bash
cargo run
```

You should see something like:

```
result: Ok("HELLO, WORLD")
steps:  2
json: {
  "id": "trace_01HX...",
  "agent": "hello",
  "steps": [
    { "name": "greet", "status": "ok", "duration_ms": 0, "confidence": 1.0 },
    { "name": "shout", "status": "ok", "duration_ms": 0, "confidence": 1.0 }
  ],
  "value": "HELLO, WORLD"
}
```

## What just happened

Three things, and each one is new vs. a hand-rolled agent:

### 1. `#[trace::agent]` injects `t`

The macro rewrites your function so that a `TraceCtx` binding called `t` is
available in scope. Every call to `t.step`, `t.delegate`, `t.speculate` is
recorded on that context. When the function returns, `t` is rolled up into a
`Trace<T>` and that's what the caller sees.

This is the same idea as Python's `contextvars` or Go's `context.Context`, but
with one important difference: **you don't have to thread it through every
function call**. The macro wires it up. If you want to call another `#[trace::agent]`
function from this one, the child's `t` automatically becomes a sub-trace of
the parent's `t`.

### 2. `t.step` is *not* just a log line

In a regular Rust agent, you'd write:

```rust
tracing::info!("greeting {}", name);
let greeting = format!("hello, {}", name);
```

That emits an event. It does not produce a value you can inspect from outside.
`t.step` does both — it runs the closure *and* records a `Step` that's now
part of the returned `Trace<T>`. You can serialize it, replay it, or branch
on it.

### 3. `Trace<T>` replaces `Result<T, E>` at the API boundary

Look at the return type: `Trace<String>`, not `Result<String, TraceErr>`.
`Trace<T>` is a wrapper that carries:

- the final value (or error)
- the causal chain of steps that produced it
- confidence scores per step
- timing per step
- any branches that were rejected along the way

You `?` through it like a `Result`, but when the function ends, the caller
gets the whole story — not just the final value.

## The mental model

If you've built agents with `agent_trace` or similar crates, you've probably
written something like this by hand:

```rust
struct AgentRun {
    id: Uuid,
    steps: Vec<StepRecord>,
    result: Option<Value>,
}

impl AgentRun {
    fn record_step(&mut self, name: &str, ...) { ... }
}
```

`trace::` is that pattern, but:

- the `AgentRun` struct is `Trace<T>`, and it's generic over your value
- the `record_step` call is `t.step`, and it runs the closure for you
- the propagation is automatic — child agents roll into parent traces
- it's serializable out of the box
- crash recovery (chapter 04) is built on top of it

## What's next

You've got the plumbing. Chapter **02** walks through the core types
(`Trace<T>`, `TraceErr`, `Step`, the `Agent` trait) — the vocabulary you'll
use for the rest of the tutorial.
