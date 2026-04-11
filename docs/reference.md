# `trace::` syntax reference card

> Every macro, trait, type, and method in one place. Use as a cheat sheet.

## Attribute macros

```rust
#[trace::agent]
async fn name(args) -> Trace<T> { ... }

#[trace::agent(registry = "reg", checkpoint_every_step)]
#[trace::agent(replay = "strict")]   // default
#[trace::agent(replay = "lenient")]  // re-run mismatches instead of failing
```

Injects a `&mut TraceCtx` binding called `t` into the function body. Wraps
the body so the return type `Trace<T>` is built from the steps recorded on
`t` plus the final value.

## `TraceCtx` methods

```rust
t.step(name, closure).await?;                       // Plain step
t.step_with_confidence(name, 0.82, closure).await?; // With explicit score

t.delegate::<Agent>(name, input)                    // Delegation builder
    .with_budget(Budget::tokens(4000))
    .on_low_confidence(0.7, handler)
    .on_step_failure(handler)
    .on_budget_exceeded(handler)
    .await?;

t.route_on_confidence(score, [                      // Confidence branching
    (0.90.., || async { ... }),
    (0.70..0.90, || async { ... }),
    (0.00..0.70, || async { ... }),
]).await?;

t.speculate(name, [                                 // Parallel alternatives
    ("cheap", || async { ... }),
    ("fast",  || async { ... }),
])
    .with_budget(Budget::tokens(8000))
    .pick_best_by(|r| r.confidence)
    .await?;
    // or: .first_ok()
    // or: .pick_best_by_racing(f)

t.on_low_confidence(0.8, handler);                  // Scoped hook
t.on_step_failure(handler);
t.on_budget_exceeded(handler);

t.budget();                                         // &Budget
t.remaining_budget();                               // u64
t.attempt();                                        // current attempt count
t.snapshot();                                       // Trace<Value> so far
```

## `Trace<T>`

```rust
pub struct Trace<T> {
    pub id: TraceId,
    pub agent: &'static str,
    pub value: Result<T, TraceErr>,
    pub steps: Vec<Step>,
    pub children: Vec<Trace<serde_json::Value>>,
    pub started_at: Instant,
    pub finished_at: Option<Instant>,
}

// Inspection
trace.value() -> Result<T, TraceErr>;
trace.causal_chain() -> Vec<&Step>;
trace.delegations() -> Vec<Delegation>;
trace.rejected_branches() -> Vec<&Step>;
trace.replay_from(snapshot) -> Trace<T>;

// Composition
Trace::join_all(futures) -> Trace<Vec<T>>;
Trace::join_all_best_effort(futures) -> Trace<Vec<Result<T, TraceErr>>>;
trace_a | trace_b  // pipe operator; chains via & output -> input
```

## `Step`

```rust
pub struct Step {
    pub name: String,
    pub kind: StepKind,
    pub status: StepStatus,
    pub confidence: f32,
    pub started_at: Instant,
    pub duration_ms: u64,
    pub input_hash: u64,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub attempt: u32,
}

pub enum StepKind { Plain, Delegation, Branch, Speculation }
pub enum StepStatus { Ok, Err, Rejected, Skipped }
```

## `TraceErr`

```rust
pub enum TraceErr {
    StepFailed { step: String, source: Box<dyn Error + Send + Sync> },
    LowConfidence { step: String, score: f32, threshold: f32 },
    BudgetExceeded { kind: BudgetKind, limit: u64, actual: u64 },
    Delegation { to: String, source: Box<TraceErr> },
    Cancelled { reason: String },
    ReplayMismatch { step: String, expected: u64, actual: u64 },
}

TraceErr::step_failed(name, msg);
TraceErr::low_confidence(name, score, threshold);
err.failed_step() -> Option<&str>;
err.is_transient() -> bool;
```

## `Agent` trait

```rust
#[async_trait]
pub trait Agent: Send + Sync + 'static {
    type Input: Serialize + DeserializeOwned + Send;
    type Output: Serialize + DeserializeOwned + Send;

    fn name() -> &'static str;
    async fn run(ctx: &mut TraceCtx, input: Self::Input)
        -> Result<Self::Output, TraceErr>;

    fn budget() -> Budget { Budget::default() }
    fn on_low_confidence(_score: f32) -> Recovery<Self::Output> { Recovery::Continue }
    fn on_step_failure(_err: &TraceErr) -> Recovery<Self::Output> { Recovery::Propagate }
}
```

## `Recovery<T>`

```rust
pub enum Recovery<T> {
    Retry,
    RetryWith(Box<dyn FnOnce() -> BoxFuture<'static, Result<T, TraceErr>>>),
    Substitute(T),
    Escalate(BoxFuture<'static, Result<T, TraceErr>>),
    Propagate,
    Skip,
    Continue,
}
```

## `Budget`

```rust
Budget::tokens(4000)
Budget::calls(20)
Budget::duration(Duration::from_secs(30))
Budget::cost_cents(500)
Budget::combined(vec![
    Budget::tokens(4000),
    Budget::duration(Duration::from_secs(30)),
])

budget.remaining() -> u64;
budget.kind() -> BudgetKind;
```

## `TaskRegistry`

```rust
TaskRegistry::in_memory();
TaskRegistry::sqlite(path);
TaskRegistry::custom(backend);

reg.submit::<S, I>(kind, input).await?           -> TaskId
reg.submit_child::<S, I>(parent, kind, input).await?
reg.get::<S>(id).await?                          -> Task<S>
reg.update_status::<S>(id, status).await?
reg.checkpoint::<T>(id, &trace).await?
reg.pending::<S>().await?                        -> Vec<Task<S>>
reg.resume::<S, A>(id).await?                    -> Trace<A::Output>
reg.children::<S>(parent).await?                 -> Vec<Task<S>>
```

## `Task<S>`

```rust
pub struct Task<S> {
    pub id: TaskId,
    pub parent: Option<TaskId>,
    pub kind: String,
    pub status: S,
    pub input: serde_json::Value,
    pub trace: Option<Trace<serde_json::Value>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub attempts: u32,
}
```

## Feature flags

```toml
trace = { version = "0.1", features = ["serde", "tokio", "sqlite", "tracing"] }
```

| Flag | Turns on |
|------|----------|
| `serde` | Serde impls for `Trace`, `Step`, `Task`, `TraceErr` |
| `tokio` | `tokio::spawn` for `delegate`, `tokio::join!` for `join_all` |
| `sqlite` | `TaskRegistry::sqlite` |
| `tracing` | Emit `tracing` events alongside `trace::` steps |
| `postgres` | `TaskRegistry::postgres` (requires `sqlx`) |

## Prelude

```rust
use trace::prelude::*;
// brings in: Trace, TraceErr, TraceCtx, Agent, Budget, Recovery,
//            Step, StepKind, StepStatus, TaskId
```
