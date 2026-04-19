# 02 — Core types

> Goal: know every type you'll see for the rest of the tutorial, and why each
> one exists as a first-class value rather than a side effect.

## The five types that matter

| Type      | Analogue you already know              | What it adds                            |
| --------- | -------------------------------------- | --------------------------------------- |
| `Crux<T>` | `Result<T, E>` + `tracing::Span` fused | Records every step as part of the value |
| `Step`    | A span event or log line               | First-class, typed, serializable        |
| `CruxErr` | `anyhow::Error`                        | Keeps the failing step in scope         |
| `Agent`   | A struct with an `async fn run`        | Declarative lifecycle hooks             |
| `CruxCtx` | `tokio::task_local!` context           | Scoped to the `#[cruxai::agent]` function  |

The rest of this chapter walks through each one.

## `Crux<T>`

```rust
pub struct Crux<T> {
    pub id: CruxId,
    pub agent: &'static str,
    pub value: Result<T, CruxErr>,
    pub steps: Vec<Step>,
    pub children: Vec<Crux<serde_json::Value>>,
    pub started_at: Instant,
    pub finished_at: Option<Instant>,
}
```

### Why the value is inside

`Crux<T>` holds the value _and_ the causal chain. You don't return a value
and separately emit a crux — they're one thing. This sounds pedantic, but it
has two downstream effects:

1. **You can't forget the crux.** If you return a `Crux<T>`, the crux goes
   with it. There's no "oh, I logged this but forgot to attach the span ID"
   class of bug.
2. **Replay is trivial.** Because the crux _is_ the return value, replaying
   a function means re-running it with a snapshot of `Crux<T>` as the seed.
   See chapter 04.

### Using it like `Result`

```rust
let t: Crux<String> = hello("world".into()).await;

// Unwrap just the value (ignores crux):
let s: String = t.value()?;

// Pattern match on the result while keeping the crux:
match t.value() {
    Ok(s) => println!("got {s} in {} steps", t.steps.len()),
    Err(e) => println!("failed at step {:?}: {e}", e.failed_step),
}
```

The `?` operator works _inside_ an `#[cruxai::agent]` function because the
macro rewrites it to propagate through `x`. Outside an agent, you call
`.value()` to extract the inner `Result`.

### Querying the crux

```rust
// Every step, in causal order:
for step in crux.causal_chain() {
    println!("{}: {:?} ({}ms)", step.name, step.status, step.duration_ms);
}

// Only the delegations:
for d in crux.delegations() {
    println!("delegated {} -> {}", d.from_agent, d.to_agent);
}

// Branches that were considered but rejected:
for r in crux.rejected_branches() {
    println!("rejected {}: confidence={}", r.name, r.confidence);
}
```

These are plain methods, not macros. They work on any `Crux<T>` you have a
reference to — including one you deserialized from JSON.

## `Step`

```rust
pub struct Step {
    pub name: String,
    pub kind: StepKind,        // Plain | Delegation | Branch | Speculation
    pub status: StepStatus,    // Ok | Err | Rejected | Skipped
    pub confidence: f32,       // 0.0..=1.0
    pub started_at: Instant,
    pub duration_ms: u64,
    pub input_hash: u64,       // for memoization / replay
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
}
```

The important fields for day-to-day work are **`kind`**, **`status`**, and
**`confidence`**.

### Why `confidence` is a built-in field

This is the biggest single departure from `tracing` / OpenTelemetry. Those
libraries treat a span as a timing primitive — it happened, here's how long.
`cruxai::` treats a step as an _epistemic_ primitive: it happened, here's how
sure we are the output is right.

That score is what powers:

- `on_low_confidence` hooks (chapter 05)
- `speculate`'s winner-picking (chapter 03)
- `TaskRegistry`'s "should we retry this?" decisions (chapter 04)

You set it with a closure return type that implements `Confidence`, or
explicitly via `x.step_with_confidence("name", 0.82, || ...)`.

## `CruxErr`

```rust
pub enum CruxErr {
    StepFailed { step: String, source: Box<dyn Error + Send + Sync> },
    LowConfidence { step: String, score: f32, threshold: f32 },
    BudgetExceeded { kind: BudgetKind, limit: u64, actual: u64 },
    Delegation { to: String, source: Box<CruxErr> },
    Cancelled { reason: String },
    ReplayMismatch { step: String, expected: u64, actual: u64 },
}
```

Three things to notice:

1. **Every variant names the failing step.** You never have to grep logs to
   figure out where a failure came from — it's in the error value.
2. **`Delegation` nests.** If an agent you delegated to failed, the error is
   `Delegation { to: "drafter", source: Box::new(StepFailed { ... }) }`. You
   can unwrap as deep as you need to know where the real fault was.
3. **`ReplayMismatch` is a real variant.** Replay isn't a library concern in
   `cruxai::` — it's a language feature, and it can fail loudly if your code
   changed in a way that invalidates a saved crux.

## `Agent` trait

```rust
#[async_trait::async_trait]
pub trait Agent: Send + Sync + 'static {
    type Input: Serialize + DeserializeOwned + Send;
    type Output: Serialize + DeserializeOwned + Send;

    fn name() -> &'static str;

    async fn run(ctx: &mut CruxCtx, input: Self::Input)
        -> Result<Self::Output, CruxErr>;

    // Optional — defaults are sensible.
    fn budget() -> Budget { Budget::default() }
    fn on_low_confidence(_score: f32) -> Recovery { Recovery::Continue }
    fn on_step_failure(_err: &CruxErr) -> Recovery { Recovery::Propagate }
}
```

You almost never implement this by hand. The `#[cruxai::agent]` attribute
generates an impl from a free function. You implement it directly only when
you need to override the lifecycle hooks at the _type_ level — usually for an
agent that's going to be delegated to from many places and needs consistent
recovery behavior.

### When to use a free function vs. an Agent impl

| Use a free function             | Use an `impl Agent`                             |
| ------------------------------- | ----------------------------------------------- |
| You call it from one place      | Many callers delegate to it                     |
| Lifecycle hooks differ per call | Lifecycle hooks are stable per agent            |
| Fast to iterate                 | You want `AgentId` in the registry (chapter 04) |

## `CruxCtx`

The `x` binding you see inside `#[cruxai::agent]` functions is of type
`&mut CruxCtx`. You don't construct it yourself — the macro does.

```rust
impl CruxCtx {
    pub async fn step<F, Fut, T>(&mut self, name: &str, f: F) -> Result<T, CruxErr>
    where F: FnOnce() -> Fut, Fut: Future<Output = Result<T, CruxErr>>;

    pub async fn delegate<A: Agent>(&mut self, name: &str, input: A::Input)
        -> DelegationBuilder<A>;

    pub fn speculate<T>(
        &mut self,
        name: &str,
        arms: impl IntoIterator<Item = (&'static str, impl FnOnce() -> BoxFuture<'static, Result<T, CruxErr>>)>,
    ) -> Speculation<T>;

    pub fn budget(&self) -> &Budget;
    pub fn remaining_budget(&self) -> u64;
    pub fn on_low_confidence(&mut self, threshold: f32, handler: impl Handler);
}
```

Three things worth knowing:

1. `x.step` takes a closure and runs it, so you can use any `async` code.
2. `x.delegate` returns a **builder**, not a future. You chain `.with_budget`,
   `.on_low_confidence`, `.on_step_failure`, and finally `.await`.
3. `x.speculate` is lazy — the arms don't run until you call a terminator
   like `.pick_best_by` or `.first_ok`.

## Check your understanding

Before moving on, make sure you can answer these:

- **Where does the value live?** _Inside `Crux<T>`, alongside the steps._
- **How do you fail a step loudly?** _Return `Err(CruxErr::StepFailed { ... })`
  from the closure, or let `?` propagate._
- **What's the difference between a `Step` and a `tracing::Event`?** _A step
  has confidence, typed output, and is part of a value you can serialize and
  replay._
- **When do you implement `Agent` directly?** _When you need stable,
  type-level lifecycle hooks across many callers._

Chapter **03** puts these types to work on branching and delegation — where
`cruxai::` diverges most from regular Rust.
