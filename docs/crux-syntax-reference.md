# `crux::` syntax reference card

## Attribute macros

```rust
#[crux::agent]
async fn name(args) -> Crux<T> { ... }

#[crux::agent(registry = "reg", checkpoint_every_step)]
#[crux::agent(replay = "strict")]   // default
#[crux::agent(replay = "lenient")]  // re-run mismatches instead of failing
```

Injects a `&mut CruxCtx` binding called `x` into the function body. Wraps
the body so the return type `Crux<T>` is built from the steps recorded on
`x` plus the final value.

## `CruxCtx` methods

```rust
// Plain step (confidence defaults to 1.0)
x.step(name, closure).await?;

// Step with explicit confidence score
x.step_with_confidence(name, 0.82, closure).await?;

// Step with content-key for replay identity
x.step_keyed(name, &key, closure).await?;

// Retryable step (closure is FnMut, retries up to max_retries)
x.step_retryable(name, 0.9, closure).await?;

// Streaming step (consumes a Stream, records intermediate events)
x.step_stream(name, || stream).await?;

// Delegation builder
x.delegate::<Agent>(name, input)
    .with_budget(Budget::tokens(4000))
    .on_low_confidence(0.7, handler)
    .on_step_failure(handler)
    .on_budget_exceeded(handler)
    .await?;

// Confidence branching (validates non-overlapping, gap-free [0.0, 1.0] coverage)
x.route_on_confidence(name, score, vec![
    (ConfidenceRange { lo: 0.90, hi: None }, "high", fut),
    (ConfidenceRange { lo: 0.70, hi: Some(0.90) }, "mid", fut),
    (ConfidenceRange { lo: 0.00, hi: Some(0.70) }, "low", fut),
]).await?;

// Sequential pipeline (each stage gets previous output)
x.pipe(name, input, vec![
    ("stage_a", |val| async { Ok(transform(val)) }),
    ("stage_b", |val| async { Ok(finish(val)) }),
]).await?;

// Parallel fan-out (arms run concurrently, results in input order)
x.join_all(name, vec![
    ("arm_a", async { Ok(result_a) }),
    ("arm_b", async { Ok(result_b) }),
]).await?;

// Speculation builder (parallel alternatives, pick best)
x.speculate(name, vec![
    ("cheap", Box::pin(async { Ok(result) })),
    ("fast",  Box::pin(async { Ok(result) })),
])
    .with_budget(Budget::tokens(8000))
    .pick_best_by(|r| r.confidence)
    .await?;
    // or: .first_ok()

// Scoped lifecycle hooks
x.on_low_confidence(0.8, handler);
x.on_step_failure(handler);
x.on_budget_exceeded(handler);

// Configuration
x.set_max_retries(5);
x.set_budget(Budget::tokens(4000));
x.consume_budget(100);

// Inspection
x.budget();            // &Budget
x.remaining_budget();  // u64
x.step_count();        // u32 (current ordinal)
x.snapshot_steps();    // &[Step]
x.snapshot();          // Crux<Value> (mid-run checkpoint)

// Replay
x.replay_from(&previous_crux);
x.set_replay_mode(ReplayMode::Lenient);

// Task registry integration
x.checkpoint_to(&registry, &task_id).await?;
x.resume_from(&registry, &task_id).await?;

// Finalize into Crux<T> (called automatically by macro)
x.finalize(result);
```

## `Crux<T>`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Crux<T> {
    pub id: CruxId,
    pub agent: String,
    pub value: Result<T, CruxErr>,
    pub steps: Vec<Step>,
    pub children: Vec<Crux<serde_json::Value>>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

// Inspection
crux.value() -> Result<&T, &CruxErr>;
crux.into_value() -> Result<T, CruxErr>;
crux.causal_chain() -> Vec<&Step>;
crux.delegations() -> Vec<Delegation<'_>>;
crux.rejected_branches() -> Vec<&Step>;
crux.duration_ms() -> Option<u64>;
crux.succeeded_count() -> usize;
crux.failed_count() -> usize;

// Checkpointing (requires T: Serialize)
crux.to_snapshot() -> Result<Crux<serde_json::Value>, serde_json::Error>;
```

## `Step`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub events: Vec<serde_json::Value>,  // streaming step intermediates
}

pub enum StepKind { Plain, Delegation, Branch, Speculation }
pub enum StepStatus { Ok, Err, Rejected, Skipped }
```

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

CruxErr::step_failed(name, msg);
CruxErr::low_confidence(name, score, threshold);
err.failed_step() -> Option<&str>;
err.is_transient() -> bool;
```

## `Agent` trait

```rust
pub trait Agent: Send + Sync + 'static {
    type Input: Serialize + DeserializeOwned + Send;
    type Output: Serialize + DeserializeOwned + Send;

    fn name() -> &'static str;
    fn run(
        ctx: &mut CruxCtx,
        input: Self::Input,
    ) -> impl Future<Output = Result<Self::Output, CruxErr>> + Send;

    fn budget() -> Budget { Budget::default() }
    fn on_low_confidence(_score: f32) -> Recovery<Self::Output> { Recovery::Continue }
    fn on_step_failure(_err: &CruxErr) -> Recovery<Self::Output> { Recovery::Propagate }
}
```

## `Recovery<T>`

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

## `Budget`

```rust
pub enum Budget {
    Tokens { limit: u64 },
    Calls { limit: u64 },
    Duration { limit_ms: u64 },
    CostCents { limit: u64 },
    Combined { budgets: Vec<Budget> },
}

Budget::tokens(4000);
Budget::calls(20);
Budget::duration(Duration::from_secs(30));
Budget::cost_cents(500);
Budget::combined(vec![...]);

budget.kind() -> BudgetKind;
budget.limit() -> u64;
```

## `TaskRegistry`

```rust
// Construction
TaskRegistry::new(backend);  // takes any RegistryBackend impl

// Available backends
InMemoryBackend::new();              // always available
RedbBackend::open(path)?;            // behind --features redb

// Operations
reg.submit(kind, input).await?           -> TaskId
reg.get(&id).await?                      -> Task
reg.update_status(&id, status).await?
reg.checkpoint(&id, &crux).await?
reg.pending(kind).await?                 -> Vec<Task>
reg.load_checkpoint(&id).await?          -> Option<Crux<Value>>
```

## `Task`

```rust
pub struct Task {
    pub id: TaskId,
    pub kind: String,
    pub status: TaskStatus,
    pub input: serde_json::Value,
    pub checkpoint: Option<Crux<serde_json::Value>>,
    pub attempts: u32,
}

pub enum TaskStatus { Pending, Running, Done, Failed }
```

## Feature flags

```toml
crux = { version = "0.3", features = ["redb", "tracing", "script"] }
```

| Flag            | Turns on                                        |
| --------------- | ----------------------------------------------- |
| `tokio-runtime` | Async support (tokio + futures). On by default. |
| `redb`          | `RedbBackend` for persistent task registry.     |
| `tracing`       | Instrument with tracing spans.                  |
| `script`        | Re-exports `crux-script` for pipeline execution. |
| `script`        | Re-export `crux-script` for pipeline execution. |

## Prelude

```rust
use crux::prelude::*;
// brings in: Agent, Context, CruxCtx, Crux, CruxErr, CruxId,
//            Step, StepKind, StepStatus, Budget, Recovery,
//            TaskRegistry, Task, TaskStatus, TaskId,
//            BoxFut, ConfidenceRange, ConfidenceRoute, JoinArm, PipeStage,
//            ReplayMode, hash_content,
//            ApprovalGate, ApprovalDecision, ApprovalRequest, RiskLevel,
//            SafetyPolicy, SafetyViolation,
//            EvolutionOutcome, HarnessDiff, HarnessProfile, ResourceHints,
//            ExecutionContext, Priority, StepState, Urgency (from slashcrux)
```
