# 04 — Serializable task management

> Goal: understand `TaskRegistry` and `Task<S>`, use them to make an agent
> crash-safe, and know exactly where the state lives at every step.

This is the chapter that makes `cruxai::` more than a logging library. Every
step, every delegation, every rejected branch in a `Crux<T>` is
serializable — so you can persist the crux, crash the process, and resume
from exactly where you left off.

## The problem we're solving

A long-running agent does this:

```
plan  ->  step 1  ->  step 2  ->  [crash]
```

When you restart, you have three bad options:

1. **Re-run from the start** — wastes tokens, risks drift if upstream state
   changed.
2. **Rely on ad-hoc checkpointing** — you end up writing custom "save after
   this step" code in every agent, and it's always slightly wrong.
3. **Ship it and pray** — the industry standard.

`cruxai::` gives you a fourth option: the runtime persists every step as it
happens, and replay is a language feature.

## The two types

### `Task<S>`

A `Task<S>` is a serializable handle to a single unit of work. `S` is your
status enum:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
enum BuildStatus {
    Queued,
    Running { started_at: DateTime<Utc> },
    Succeeded { artifact_url: String },
    Failed { error: String },
    AwaitingApproval { human: String },
}

// The task itself:
pub struct Task<S> {
    pub id: TaskId,
    pub parent: Option<TaskId>,
    pub kind: String,          // "build", "deploy", "research", etc.
    pub status: S,
    pub input: serde_json::Value,
    pub crux: Option<Crux<serde_json::Value>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub attempts: u32,
}
```

The key fields:

- **`status: S`** — your own type. The runtime doesn't care what the states
  are; it just persists the enum. This is where `cruxai::` leans on Rust's
  type system instead of inventing its own status primitives.
- **`crux: Option<Crux<...>>`** — the crux so far. On restart, this is
  the seed for replay.
- **`attempts`** — the runtime bumps this every time a `Failed` task gets
  retried. Your lifecycle hooks can inspect it.

### `TaskRegistry`

```rust
pub struct TaskRegistry { ... }

impl TaskRegistry {
    pub fn in_memory() -> Self;
    pub fn sqlite(path: &Path) -> Result<Self, RegistryErr>;
    pub fn custom<B: RegistryBackend>(backend: B) -> Self;

    pub async fn submit<S, I>(&self, kind: &str, input: I) -> Result<TaskId, RegistryErr>
    where S: Default + Serialize + DeserializeOwned, I: Serialize;

    pub async fn get<S>(&self, id: TaskId) -> Result<Task<S>, RegistryErr>;
    pub async fn update_status<S>(&self, id: TaskId, status: S) -> Result<(), RegistryErr>;

    pub async fn checkpoint<T: Serialize>(&self, id: TaskId, crux: &Crux<T>) -> Result<(), RegistryErr>;

    pub async fn pending<S>(&self) -> Result<Vec<Task<S>>, RegistryErr>;
    pub async fn resume<S, A: Agent>(&self, id: TaskId) -> Result<Crux<A::Output>, CruxErr>;
}
```

Three backends ship in-tree:

| Backend     | When to use                                                       |
| ----------- | ----------------------------------------------------------------- |
| `in_memory` | Tests, single-process agents, experiments                         |
| `sqlite`    | Single host, crash-safe, embeddable (enable the `sqlite` feature) |
| `custom`    | You bring Postgres, Redis, DynamoDB — implement `RegistryBackend` |

`RegistryBackend` is a small trait (`get`, `put`, `list`, `cas`). You almost
never need to write one yourself.

## Wiring it into an agent

Here's a real example — the scaffolding for a build-and-deploy agent:

```rust
use cruxai::prelude::*;
use cruxai::registry::{TaskRegistry, Task, TaskId};
use serde::{Serialize, Deserialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
enum DeployStatus {
    #[default]
    Queued,
    Building,
    Testing,
    Deploying,
    Succeeded { url: String },
    Failed { error: String, step: String },
}

#[derive(Serialize, Deserialize)]
struct DeployInput {
    repo: String,
    ref_: String,
    env: String,
}

#[cruxai::agent(registry = "reg", checkpoint_every_step)]
async fn deploy(reg: &TaskRegistry, task_id: TaskId, input: DeployInput)
    -> Crux<String>
{
    reg.update_status::<DeployStatus>(task_id, DeployStatus::Building).await?;
    let artifact = t.step("build", || build(&input.repo, &input.ref_)).await?;

    reg.update_status(task_id, DeployStatus::Testing).await?;
    t.step("test", || run_tests(&artifact)).await?;

    reg.update_status(task_id, DeployStatus::Deploying).await?;
    let url = t.step("deploy", || deploy_to(&input.env, &artifact)).await?;

    reg.update_status(task_id, DeployStatus::Succeeded { url: url.clone() }).await?;
    Ok(url)
}
```

A few things to notice:

### `registry = "reg"` and `checkpoint_every_step`

The macro takes two optional arguments:

- `registry = "reg"` — points at the `TaskRegistry` binding in scope. The
  macro injects `reg.checkpoint(task_id, &t.snapshot()).await?` after every
  step (when `checkpoint_every_step` is set).
- `checkpoint_every_step` — the default is "checkpoint at delegation
  boundaries only." That's fast and usually enough. Turn on per-step
  checkpointing when the steps are expensive and you _really_ want crash
  safety between them.

If you need something custom, omit both and call `reg.checkpoint(task_id,
&t.snapshot()).await?` explicitly at the points you care about. `t.snapshot()`
returns a `Crux<serde_json::Value>` that's a live view of the crux so far.

### `update_status` is separate from `checkpoint`

Two different things, easy to confuse:

- **`update_status`** changes your `Task<S>::status` field — the business-
  level state machine.
- **`checkpoint`** persists the `Crux<T>` — the execution history.

Typically you call `update_status` when your business logic advances (about
to start building), and `checkpoint` when the _runtime_ wants to save
progress. With `checkpoint_every_step`, the macro handles the second one.

## How replay actually works

When you call `reg.resume::<DeployStatus, DeployAgent>(task_id)`, here's
what the runtime does:

1. Load the `Task<DeployStatus>` from the backend.
2. Read `task.crux` — the `Crux<Value>` snapshot.
3. Start a new `CruxCtx` seeded from that snapshot.
4. Re-run the agent function. For every `t.step("name", ...)`:
   - Compute the input hash.
   - If the snapshot has a step with the same name and matching input hash,
     skip the closure entirely and return the recorded output.
   - Otherwise, run the closure fresh and record a new step.
5. Return a `Crux<T>` with the reconstructed history plus any new work.

The skip-if-input-matches step is what makes replay _correct_ — not just
fast. If you changed the code between crash and restart in a way that makes
step 2's input different, the input hashes won't match, the closure re-runs,
and the crux records a new step. Correctness first, speed second.

### What fails at replay time

Replay is strict by default. You'll get a `CruxErr::ReplayMismatch` in these
cases:

| Situation                   | Why it fails                                     |
| --------------------------- | ------------------------------------------------ |
| You renamed a step          | Can't correlate old step to new step             |
| You reordered steps         | Causal chain no longer matches                   |
| A step's input hash changed | Would return stale output                        |
| A delegation target changed | Old sub-crux can't be replayed against new agent |

You can loosen this with `#[cruxai::agent(replay = "lenient")]` which will
re-run mismatched steps instead of failing — but default-strict is the right
default. An agent that silently replays the wrong crux is worse than one
that refuses to replay at all.

## The full lifecycle

Put it together and a task goes through six observable states:

```
Submitted  ->  Running  ->  Checkpointed  ->  [crash]
                                                 |
                                                 v
                                              Resumed  ->  Checkpointed  ->  Completed
```

The registry has every one of those transitions persisted. You can build a
dashboard, a retry policy, an SLO monitor, or a human-in-the-loop queue on
top of it without any additional plumbing.

## In-memory vs SQLite: when each makes sense

```rust
// Tests:
let reg = TaskRegistry::in_memory();

// Single-host service:
let reg = TaskRegistry::sqlite(Path::new("./tasks.db"))?;
```

The in-memory registry is _not_ just for tests — it's genuinely useful when
you want crux/replay semantics inside a single process without persistence.
Example: a long-running CLI that wants to retry failed steps but doesn't
need to survive a restart.

SQLite is the default for anything you'd actually ship. One file, zero
operational overhead, supports concurrent readers, and `serde_json::Value`
columns mean you don't need schema migrations when your `Task<S>::S` enum
changes.

## Check your understanding

- **Where does the `S` in `Task<S>` come from?** _You define it. The runtime
  just persists it._
- **What's the difference between `update_status` and `checkpoint`?**
  _Status is business-level; checkpoint is execution history._
- **What does `checkpoint_every_step` do?** _Tells the macro to persist the
  crux after every `t.step` call, not just at delegation boundaries._
- **What makes replay correct rather than just fast?** _Input hashes — a
  step only gets skipped if its input matches the recorded one._

Chapter **05** covers the lifecycle hooks (`on_low_confidence`,
`on_step_failure`, `on_budget_exceeded`) that let you recover gracefully
instead of just crashing into the registry.
