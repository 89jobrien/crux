# Chapter 01 — Setup and First Agent

This chapter walks from zero to a working agent. By the end you will have a project that compiles,
runs a two-step agent, and prints a structured trace as JSON.

---

## Toolchain Requirements

Cruxx requires **Rust 1.85 or later** and **edition 2024**.

The two key language features that make the programming model work are:

- **Native async fn in traits** (stabilized in 1.75, fully ergonomic in 1.85). The `Agent` trait
  uses `async fn run(...)` directly. There is no `#[async_trait]` macro in this codebase and none
  is needed in yours.
- **Edition 2024 resolver semantics**. Feature unification behaves differently in edition 2024.
  Use it in every crate that depends on `cruxx`.

Check your toolchain:

```bash
rustup show active-toolchain
# should print: stable-aarch64-apple-darwin (or similar) with version >= 1.85
```

If you are on an older toolchain:

```bash
rustup update stable
```

---

## Scaffolding a Project

```bash
cargo new hello-cruxx
cd hello-cruxx
```

Open `Cargo.toml` and add the dependencies:

```toml
[package]
name    = "hello-cruxx"
version = "0.1.0"
edition = "2024"

[dependencies]
cruxx      = { version = "0.2", features = ["tokio-runtime"] }
tokio      = { version = "1", features = ["full"] }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
```

### Feature flags

| Flag            | Default | Purpose                                              |
|-----------------|---------|------------------------------------------------------|
| `tokio-runtime` | yes     | Async support. Required for `x.step` to compile.    |
| `redb`          | no      | `RedbBackend` — embedded key-value registry storage. |
| `tracing`       | no      | Instrument steps with `tracing` spans.               |

`tokio-runtime` is on by default, so listing it explicitly is optional. The example above includes
it for clarity.

Serde support is always-on — there is no feature flag for it. `Crux<T>` derives `Serialize` and
`Deserialize` unconditionally, so the `serde` and `serde_json` entries in your `Cargo.toml` are
all you need to serialize traces to JSON.

---

## A Minimal Agent

Replace the contents of `src/main.rs`:

```rust
use cruxx::prelude::*;

#[cruxx::agent]
async fn hello(name: String) -> Crux<String> {
    let greeting = x.step("greet", || async {
        Ok(format!("hello, {}", name))
    }).await?;

    let shouted = x.step("shout", || async {
        Ok(greeting.to_uppercase())
    }).await?;

    Ok(shouted)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let result = hello("world".into()).await;

    println!("result: {:?}", result.value());
    println!("steps:  {}", result.causal_chain().len());
    println!("json:   {}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
```

Run it:

```bash
cargo run
```

Expected output (abbreviated):

```
result: Ok("HELLO, WORLD")
steps:  2
json:   {
  "id": "...",
  "agent": "hello",
  "steps": [
    { "name": "greet", "kind": "plain", "status": "ok", "confidence": 1.0, "duration_ms": 0 },
    { "name": "shout", "kind": "plain", "status": "ok", "confidence": 1.0, "duration_ms": 0 }
  ],
  "value": { "Ok": "HELLO, WORLD" }
}
```

---

## What Just Happened

There are three things worth understanding before moving forward.

### 1. The macro injects `x`

`#[cruxx::agent]` transforms the async function. It injects a `&mut CruxCtx` variable named `x`
into the function body and wraps the whole thing in glue that creates the context, drives
execution, and calls `finalize()` to produce the `Crux<T>`.

You do not declare `x` — it appears automatically. Every agent gets exactly one context, and every
step call goes through it.

### 2. `x.step` records a `Step`

```rust
let greeting = x.step("greet", || async {
    Ok(format!("hello, {}", name))
}).await?;
```

This does three things:

- Runs the closure asynchronously.
- Records a `Step` into the trace: name, kind, status, confidence, timestamps, duration, and the
  output value.
- Returns the inner `Ok(...)` value so you can bind it to `greeting`.

The `?` propagates errors through the agent in the normal Rust way. A failed step sets
`status: "err"` in the trace before propagating.

Each `Step` carries these fields:

| Field          | Type              | Description                                           |
|----------------|-------------------|-------------------------------------------------------|
| `name`         | `String`          | The label passed to `x.step(...)`.                    |
| `kind`         | `StepKind`        | `plain`, `delegation`, `speculation`, etc.            |
| `status`       | `StepStatus`      | `ok`, `err`, `skipped`, `rejected`.                   |
| `confidence`   | `f64`             | 0.0–1.0 score; defaults to 1.0 for plain steps.       |
| `started_at`   | `DateTime<Utc>`   | Wall-clock start time (chrono, not `Instant`).        |
| `duration_ms`  | `u64`             | Elapsed time for this step.                           |
| `input_hash`   | `u64`             | Hash of the step name and ordinal.                    |
| `content_hash` | `Option<u64>`     | Hash of the output value, if present.                 |
| `output`       | `Option<Value>`   | Serialized output value.                              |
| `error`        | `Option<String>`  | Error message, if the step failed.                    |
| `attempt`      | `u32`             | Retry count; 0 for first attempt.                     |
| `events`       | `Vec<StepEvent>`  | Sub-events emitted during step execution.             |

### 3. `Crux<T>` replaces `Result<T, E>`

The return type of an agent is `Crux<T>`, not `Result<T, E>`. It is a fused value-and-trace:

| Field          | Type                    | Description                                      |
|----------------|-------------------------|--------------------------------------------------|
| `id`           | `CruxId`                | Unique run identifier (UUID).                    |
| `agent`        | `String`                | Name of the agent that produced this value.      |
| `value`        | `Result<T, CruxErr>`    | The final output or error.                       |
| `steps`        | `Vec<Step>`             | All steps recorded during this run.              |
| `children`     | `Vec<Crux<Value>>`      | Sub-traces from delegations or speculation.      |
| `started_at`   | `DateTime<Utc>`         | Wall-clock start time.                           |
| `finished_at`  | `Option<DateTime<Utc>>` | Wall-clock end time (set by `finalize()`).       |

Timestamps are `DateTime<Utc>` from the `chrono` crate, not `std::time::Instant`. They serialize
to ISO 8601 strings and are suitable for storage, comparison, and cross-process replay.

Key methods:

- `result.value()` — the `Result<&T, &CruxErr>` outcome.
- `result.causal_chain()` — flat list of all steps in execution order.
- `result.delegations()` — child traces produced by `x.delegate(...)`.
- `result.rejected_branches()` — speculation branches that lost the race.

---

## Mental Model: Hand-Rolled vs. Cruxx

Without Cruxx, a typical agent looks like this:

```rust
async fn hello(name: String) -> Result<String, MyError> {
    let greeting = format!("hello, {}", name);
    let shouted  = greeting.to_uppercase();
    Ok(shouted)
}
```

This is perfectly fine Rust. What it cannot do is answer questions after the fact:

- Which steps ran? In what order?
- How long did each one take?
- What was the intermediate value at each stage?
- Did a downstream caller delegate to this agent, and what did it pass?

Cruxx makes the execution structure inspectable by turning the implicit call stack into an explicit
`Vec<Step>` that travels alongside the value. A caller that receives `Crux<String>` gets both the
result and the full audit trail, serializable to JSON, storable in a registry, and replayable
deterministically.

The tradeoff is that you write `x.step("name", || async { ... })` instead of a bare expression.
The macro keeps the overhead syntactic — the underlying async machinery is identical.

If you have built agents with hand-rolled task queues, you have likely written something like:

```rust
struct AgentRun {
    id: Uuid,
    steps: Vec<StepRecord>,
    result: Option<Value>,
}
```

`Crux<T>` is that pattern made generic, serializable, and composable. Child agents roll their
traces into parent traces automatically. The `TaskRegistry` (chapter 04) adds durable storage and
checkpoint/resume on top.

---

## Next Chapter

Chapter 02 covers the core types in depth: `Crux<T>`, `CruxErr`, `Step`, and the `Agent` trait —
the vocabulary used throughout the rest of the tutorial.

-> [02 — Core Types](./02-core-types.md)
