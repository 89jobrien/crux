# 02 — Core types

> Goal: know every type you'll see for the rest of the tutorial, and why each one exists as a
> first-class value rather than a side effect.

## The five types that matter

| Type      | Analogue you already know              | What it adds                             |
| --------- | -------------------------------------- | ---------------------------------------- |
| `Crux<T>` | `Result<T, E>` + `tracing::Span` fused | Records every step as part of the value  |
| `Step`    | A span event or log line               | First-class, typed, serializable         |
| `CruxErr` | `anyhow::Error`                        | Keeps the failing step in scope          |
| `Agent`   | A struct with an `async fn run`        | Declarative, type-level lifecycle hooks  |
| `CruxCtx` | `tokio::task_local!` context           | Scoped to the `#[cruxx::agent]` function |

The rest of this chapter walks through each one.

---

## `Crux<T>`

```rust
pub struct Crux<T> {
    pub id: CruxId,
    pub agent: String,
    pub value: Result<T, CruxErr>,
    pub steps: Vec<Step>,
    pub children: Vec<Crux<serde_json::Value>>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}
```

`agent` is a heap-allocated `String`, not `&'static str` — agent names are resolved at runtime
from the registry. `started_at` and `finished_at` are `DateTime<Utc>` (not `Instant`) so the
trace is serializable and survives process boundaries.

### Why the value is inside

`Crux<T>` holds the value _and_ the causal chain. You do not return a value and separately emit
a trace — they are one thing. This has two concrete effects:

1. **You cannot forget the trace.** If you return a `Crux<T>`, the trace goes with it. There
   is no "I logged it but forgot to attach the span ID" class of bug.
2. **Replay is trivial.** Because the trace _is_ the return value, replaying a function means
   re-running it with a snapshot of `Crux<T>` as the seed. See chapter 04.

### Using it like `Result`

```rust
let result: Crux<String> = hello("world".into()).await;

// Extract just the inner Result (borrows):
let s: &str = result.value()?.as_str();

// Consume the Crux and take ownership of the value:
let s: String = result.into_value()?;

// Pattern-match while keeping the trace in scope:
match result.value() {
    Ok(s) => println!("got {s} in {} steps", result.steps.len()),
    Err(e) => println!("failed: {e:?}"),
}
```

The `?` operator works _inside_ an `#[cruxx::agent]` function because the macro rewrites it
to propagate through the context. Outside an agent, call `.value()` or `.into_value()` to
extract the inner `Result`.

### Querying the trace

```rust
// Every step in causal order (includes child agent steps):
for step in result.causal_chain() {
    println!("{}: {:?} ({}ms)", step.name, step.status, step.duration_ms);
}

// Only delegation steps:
for d in result.delegations() {
    println!("delegated to {}", d.name);
}

// Speculation arms that were considered but not selected:
for r in result.rejected_branches() {
    println!("rejected {}: confidence={}", r.name, r.confidence);
}

// Aggregate metrics:
println!("succeeded: {}, failed: {}", result.succeeded_count(), result.failed_count());
println!("wall time: {}ms", result.duration_ms());

// Snapshot for persistence or replay:
let snap = result.to_snapshot();
```

These are plain methods, not macros. They work on any `Crux<T>` you hold a reference to,
including one deserialized from JSON.

---

## `Step`

```rust
pub struct Step {
    pub name: String,
    pub kind: StepKind,
    pub status: StepStatus,
    pub confidence: f32,
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub input_hash: u64,
    pub content_hash: Option<u64>,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub attempt: u32,
    pub events: Vec<serde_json::Value>,
}
```

`events` is skipped during serialization when empty (`#[serde(default, skip_serializing_if =
"Vec::is_empty")]`), so it does not bloat trace payloads on the happy path.

### Enumerations

```rust
pub enum StepKind {
    Plain,
    Delegation,
    Branch,
    Speculation,
}

pub enum StepStatus {
    Ok,
    Err,
    Rejected,
    Skipped,
}
```

### Why `confidence` is a built-in field

This is the most significant departure from `tracing` / OpenTelemetry. Those libraries treat a
span as a timing primitive — it happened, here is how long it took. `cruxx::` treats a step as
an _epistemic_ primitive: it happened, here is how sure we are the output is correct.

That score drives three independent mechanisms:

- `on_low_confidence` hooks on the `Agent` trait (chapter 05)
- `speculate`'s winner selection via `pick_best_by` (chapter 03)
- `TaskRegistry` retry decisions based on accumulated confidence (chapter 04)

You set it explicitly with `ctx.step_with_confidence("name", 0.82, || ...)` or via `attempt`
for retryable steps recorded with `ctx.step_retryable`.

---

## `CruxErr`

```rust
pub enum CruxErr {
    StepFailed { step: String, source_msg: String },
    LowConfidence { step: String, score: f32, threshold: f32 },
    BudgetExceeded { budget_kind: BudgetKind, limit: u64, actual: u64 },
    Delegation { to: String, source: Box<CruxErr> },
    Cancelled { reason: String },
    ReplayMismatch { step: String, expected: u64, actual: u64 },
}
```

`StepFailed` carries the error as a `String` (`source_msg`), not a `Box<dyn Error>`. This keeps
`CruxErr` serializable across process boundaries without any special trait plumbing.

Three things to notice:

1. **Every variant names the failing step.** You never have to search logs to determine where a
   failure originated — it is in the error value.
2. **`Delegation` nests.** When an agent you delegated to failed, the error is
   `Delegation { to: "drafter", source: Box::new(StepFailed { ... }) }`. Unwrap as deep as
   needed to locate the root fault.
3. **`ReplayMismatch` is a first-class variant.** Replay is a language feature in `cruxx::`,
   not a library concern. It fails loudly when your code changes in a way that invalidates a
   saved trace.

### Helper methods

```rust
// Constructors:
CruxErr::step_failed("parse", "unexpected EOF")
CruxErr::low_confidence("classify", 0.41, 0.70)

// Introspection:
err.failed_step()    // -> Option<&str>  the step name, if applicable
err.is_transient()   // -> bool          safe to retry?
```

---

## `Agent` trait

The `Agent` trait uses native return-position `impl Trait` in trait (RPITIT) — there is no
`async_trait` macro involved.

```rust
pub trait Agent: Send + Sync + 'static {
    type Input: Serialize + DeserializeOwned + Send;
    type Output: Serialize + DeserializeOwned + Send;

    fn name() -> &'static str;

    fn run(
        ctx: &mut CruxCtx,
        input: Self::Input,
    ) -> impl Future<Output = Result<Self::Output, CruxErr>> + Send;

    fn budget() -> Budget {
        Budget::default()
    }

    fn on_low_confidence(_score: f32) -> Recovery<Self::Output> {
        Recovery::Continue
    }

    fn on_step_failure(_err: &CruxErr) -> Recovery<Self::Output> {
        Recovery::Propagate
    }
}
```

You almost never write this impl by hand. The `#[cruxx::agent]` attribute generates the impl
from a free `async fn`. You implement it directly only when you need to override the lifecycle
hooks at the _type_ level — typically for an agent delegated to from many callers that requires
consistent recovery behavior everywhere.

### When to use a free function vs. an `Agent` impl

| Use a free function             | Use an `impl Agent`                             |
| ------------------------------- | ----------------------------------------------- |
| Called from one place           | Many callers delegate to it                     |
| Lifecycle hooks differ per call | Lifecycle hooks are stable per agent type       |
| Rapid iteration                 | You want a registered `AgentId` (chapter 04)    |
| No registry involvement         | Agent appears in `TaskRegistry` submissions     |

---

## `CruxCtx`

The `ctx` binding inside `#[cruxx::agent]` functions is of type `&mut CruxCtx`. You do not
construct it yourself — the macro does, then calls `finalize()` to produce the `Crux<T>`.

`CruxCtx` implements the `Context` trait, which is the DIP abstraction used in tests to inject
mock contexts. The trait surface covers: `step`, `step_keyed`, `step_with_confidence`,
`step_retryable`, `step_stream`, `on_low_confidence`, `on_step_failure`, `on_budget_exceeded`,
`set_max_retries`, `set_budget`, `consume_budget`, `budget`, `remaining_budget`, `step_count`,
and `snapshot_steps`.

The key public methods on `CruxCtx` itself:

```rust
impl CruxCtx {
    // Construct directly (tests, composition root):
    pub fn new(agent: &str, budget: Budget) -> Self;

    // Record a unit of work:
    pub async fn step<F, Fut, T>(&mut self, name: &str, f: F) -> Result<T, CruxErr>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, CruxErr>> + Send;

    // Delegate to another Agent (returns a builder):
    pub fn delegate<A: Agent>(&mut self, input: A::Input) -> DelegationBuilder<A>;

    // Fan out to competing branches (returns a builder):
    pub fn speculate<T>(&mut self, name: &str) -> SpeculationBuilder<T>;

    // Sequential pipeline:
    pub async fn pipe<T>(&mut self, name: &str, ...) -> Result<T, CruxErr>;

    // Parallel fan-out (futures::join_all):
    pub async fn join_all<T>(&mut self, name: &str, ...) -> Result<Vec<T>, CruxErr>;

    // Confidence-based routing:
    pub async fn route_on_confidence<T>(...) -> Result<T, CruxErr>;

    // Budget introspection:
    pub fn budget(&self) -> &Budget;
    pub fn remaining_budget(&self) -> u64;

    // Persistence and replay:
    pub async fn checkpoint_to(&mut self, backend: &impl RegistryBackend) -> Result<(), CruxErr>;
    pub async fn resume_from(&mut self, snapshot: &CruxSnapshot) -> Result<(), CruxErr>;
    pub async fn replay_from(&mut self, snapshot: &CruxSnapshot) -> Result<(), CruxErr>;
    pub fn set_replay_mode(&mut self, mode: ReplayMode);

    // Finalize after agent completes:
    pub fn finalize<T>(self, value: Result<T, CruxErr>) -> Crux<T>;

    // Read-only snapshot of current trace:
    pub fn snapshot(&self) -> CruxSnapshot;
}
```

Three things worth knowing before you use it:

1. `ctx.step` takes a closure and runs it, so any `async` code goes inside. Errors returned
   from the closure are wrapped in `CruxErr::StepFailed` automatically.
2. `ctx.delegate` returns a **builder** (`DelegationBuilder`), not a future. Chain
   `.with_budget(...)`, `.on_low_confidence(...)`, `.on_step_failure(...)`, then `.await`.
3. `ctx.speculate` is lazy — arms do not execute until you call a terminator
   (`.pick_best_by`, `.first_ok`, or `.race`). Losing arms are marked `Rejected`.

---

## Supporting types

### `Recovery<T>`

Returned from `on_low_confidence` and `on_step_failure` hooks to control what happens next:

```rust
pub enum Recovery<T> {
    Retry,
    RetryWith(Box<dyn FnOnce() -> BoxFut<T> + Send>),
    Substitute(T),
    Escalate(BoxFut<T>),
    Propagate,
    Skip,
    Continue,
}
```

`Continue` means "ignore the low confidence score and proceed with the value as-is." `Propagate`
means "treat this as a fatal error and surface it to the caller."

### `Budget`

```rust
pub enum Budget {
    Tokens { limit: u64 },
    Calls { limit: u64 },
    Duration { limit_ms: u64 },
    CostCents { limit: u64 },
    Combined { budgets: Vec<Budget> },
}
```

Constructors: `Budget::tokens(n)`, `Budget::calls(n)`, `Budget::duration(d)`,
`Budget::cost_cents(n)`, `Budget::combined(vec![...])`.

The default is `Budget::Tokens { limit: u64::MAX }` — effectively unlimited. Override it in
`Agent::budget()` or per-delegation via `DelegationBuilder::with_budget(...)`. The runtime
enforces limits through `BudgetTracker` and raises `CruxErr::BudgetExceeded` on violation.

---

Chapter **03** puts these types to work on branching and delegation — where `cruxx::` diverges
most from regular Rust.
