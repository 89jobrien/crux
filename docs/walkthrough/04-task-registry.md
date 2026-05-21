# 04 — Task registry: crash-safe agents

> Goal: understand `Task` and `TaskRegistry<B>`, wire them into an agent, and know exactly
> what happens at replay time — including what fails and why.

This is the chapter that makes crux more than a logging library. Every step, every delegation,
every rejected branch in a `Crux<T>` is serializable. Persist the trace, crash the process,
and resume from exactly where you left off.

## The problem

A long-running agent does this:

```text
plan  ->  step 1  ->  step 2  ->  [crash]
```

On restart you have three bad options:

1. Re-run from the start — wastes tokens, risks drift if upstream state changed.
2. Ad-hoc checkpointing — custom "save after this step" code in every agent, always slightly wrong.
3. Ship it and pray.

Cruxx gives you a fourth: the runtime persists every step as it happens, and replay is a language
feature, not a library bolted on afterward.

## The two types

### `Task`

A `Task` is a serializable handle to a single unit of work.

```rust
pub enum TaskStatus {
    Pending,
    Running,
    Done,
    Failed,
}

pub struct Task {
    pub id: TaskId,
    pub kind: String,
    pub status: TaskStatus,
    pub input: serde_json::Value,
    pub checkpoint: Option<Crux<serde_json::Value>>,
    pub attempts: u32,
}
```

Key fields:

- **`status`** — a fixed four-variant enum. The runtime advances it; your agent reads it.
- **`checkpoint`** — the execution trace so far. On restart this is the seed for replay.
- **`attempts`** — bumped every time a `Failed` task gets retried. Lifecycle hooks can inspect it.
- **`kind`** — a plain string tag ("build", "research", "deploy"). Used by `pending` for filtering.

Note what `Task` does _not_ have: no parent task, no timestamp fields, no generic status parameter.
The status type is fixed. If you need richer state, put it in `input` or encode it in `kind`.

### `TaskRegistry<B>`

```rust
pub struct TaskRegistry<B> { backend: B }

impl<B: RegistryBackend> TaskRegistry<B> {
    pub fn new(backend: B) -> Self;

    pub async fn submit<I: Serialize>(
        &self,
        kind: &str,
        input: I,
    ) -> Result<TaskId, RegistryErr>;

    pub async fn get(&self, id: &TaskId) -> Result<Task, RegistryErr>;

    pub async fn update_status(
        &self,
        id: &TaskId,
        status: TaskStatus,
    ) -> Result<(), RegistryErr>;

    pub async fn checkpoint<T: Serialize>(
        &self,
        id: &TaskId,
        crux: &Crux<T>,
    ) -> Result<(), RegistryErr>;

    pub async fn pending(&self, kind: &str) -> Result<Vec<Task>, RegistryErr>;

    pub async fn load_checkpoint(
        &self,
        id: &TaskId,
    ) -> Result<Option<Crux<serde_json::Value>>, RegistryErr>;
}
```

A few things worth calling out:

- `update_status` and `checkpoint` both use a bounded compare-and-swap retry (3 attempts) to avoid
  lost updates under concurrent writes.
- `pending` takes a `kind` filter. Pass `""` to return all pending tasks regardless of kind.
- There is no `resume` method on `TaskRegistry`. Resuming is a two-step operation: call
  `load_checkpoint` to get the trace, then call `ctx.resume_from` or `ctx.replay_from` on the
  context. This keeps the registry as a pure store and puts replay logic where it belongs — on
  `CruxCtx`.

## Backends

`RegistryBackend` is a four-method trait (`get`, `put`, `list`, `cas`). Two adapters ship in-tree:

| Backend           | When to use                                                    |
| ----------------- | -------------------------------------------------------------- |
| `InMemoryBackend` | Tests, single-process agents, experiments — always available   |
| `RedbBackend`     | Single-host crash-safe persistence — enable the `redb` feature |

There is no SQLite backend. Redb is a pure-Rust embedded key-value store with no C dependency and
no schema. It is the right default for anything you would actually ship on a single host.

Constructing a registry:

```rust
use crux::registry::{TaskRegistry, InMemoryBackend};

// Tests and in-process use:
let reg = TaskRegistry::new(InMemoryBackend::new());
```

```rust
// With the `redb` feature:
use crux::registry::{TaskRegistry, RedbBackend};
use std::path::Path;

let reg = TaskRegistry::new(RedbBackend::open(Path::new("./tasks.redb"))?);
```

If you need Postgres, Redis, or DynamoDB, implement `RegistryBackend` for your client type and
pass it to `TaskRegistry::new`. You almost never need to do this.

## Wiring a registry into an agent

Here is a build-and-deploy agent that uses the registry for crash safety:

```rust
use crux::prelude::*;
use crux::registry::{TaskRegistry, InMemoryBackend, TaskId, TaskStatus};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct DeployInput {
    repo: String,
    git_ref: String,
    env: String,
}

#[crux::agent(registry = "reg", checkpoint_every_step)]
async fn deploy(
    reg: &TaskRegistry<InMemoryBackend>,
    task_id: TaskId,
    input: DeployInput,
) -> Crux<String> {
    reg.update_status(&task_id, TaskStatus::Running).await?;

    let artifact = x.step("build", || build(&input.repo, &input.git_ref)).await?;
    let _report  = x.step("test",  || run_tests(&artifact)).await?;
    let url      = x.step("deploy", || deploy_to(&input.env, &artifact)).await?;

    reg.update_status(&task_id, TaskStatus::Done).await?;
    Ok(url)
}
```

### `registry = "reg"` and `checkpoint_every_step`

The macro takes two optional attributes:

- `registry = "reg"` — names the `TaskRegistry` binding in scope. The generated `run_registered`
  method on `DeployAgent` handles submitting the task and threading the `task_id` through.
- `checkpoint_every_step` — injects `reg.checkpoint(&task_id, &x.snapshot()).await?` after every
  `x.step` call. The default (without this attribute) checkpoints only at delegation boundaries.
  Turn it on when steps are expensive and you need crash safety between them.

To checkpoint manually at specific points:

```rust
// x.snapshot() returns Crux<serde_json::Value> — a live view of the trace
reg.checkpoint(&task_id, &x.snapshot()).await?;
```

### `update_status` is separate from `checkpoint`

These do different things:

- **`update_status`** advances `Task::status` — the coarse business-level state machine (Pending,
  Running, Done, Failed).
- **`checkpoint`** persists `Task::checkpoint` — the full execution history as a `Crux<Value>`.

Call `update_status` when your business logic transitions. The macro (or explicit calls) handles
`checkpoint` for the execution history.

## Resuming after a crash

There is no single `resume` call. You load the checkpoint, then seed the context:

```rust
// Load the previous trace:
let previous: Option<Crux<Value>> = reg.load_checkpoint(&task_id).await?;

if let Some(trace) = previous {
    // Seed replay from the loaded trace:
    ctx.replay_from(&trace);
}
```

Or, if you have a context already associated with a registry:

```rust
// snapshot + persist in one call:
x.checkpoint_to(&reg, &task_id).await?;

// load + seed replay in one call:
x.resume_from(&reg, &task_id).await?;
```

Once the replay cache is seeded, re-run the agent function normally. Steps whose name and input
hash match the cache are skipped; the recorded output is returned immediately. Steps that do not
match run fresh.

## How replay works

Steps are matched by name combined with an ordinal hash (`hash_step_identity`). For each
`x.step("name", closure)` call during a resumed run:

1. Compute the input hash for this invocation.
2. Look up a cached step with the same name and ordinal.
3. If found and the input hash matches, return the cached output immediately — no closure call.
4. If not found or the input hash does not match, run the closure and record a new step.

The input-hash check is what makes replay _correct_, not just fast. If code changed between crash
and restart such that a step's input is now different, the closure re-runs and the trace records a
new step. Correctness is the contract; speed is the consequence.

### Two replay modes

**Strict (default)** — any mismatch raises `CruxErr::ReplayMismatch`. The run stops. You inspect
the trace, fix the mismatch, and decide explicitly whether to replay or start fresh.

**Lenient** — a mismatch triggers a forward name scan through the cache instead of failing.
Ordinal shifts are the designed recovery path in this mode, not an error. Enable it per-agent:

```rust
#[crux::agent(replay = "lenient")]
async fn my_agent(input: Input) -> Crux<Output> { ... }
```

Or at runtime on the context:

```rust
ctx.set_replay_mode(ReplayMode::Lenient);
```

Default-strict is the right default. An agent that silently replays the wrong trace is harder to
debug than one that refuses to replay at all.

### What fails at replay time

Under strict mode, `CruxErr::ReplayMismatch` is raised in these cases:

| Situation                 | Why it fails                                          |
| ------------------------- | ----------------------------------------------------- |
| Step was renamed          | Ordinal + name lookup finds no matching cache entry   |
| Steps were reordered      | Causal chain no longer aligns with the recorded trace |
| Step's input changed      | Would return stale output for new input               |
| Delegation target changed | Child trace cannot be replayed against a new agent    |

## The full lifecycle

```text
submit()
    |
    v
 Pending  -->  Running  -->  [checkpoint]  -->  Done
                  |                 |
                  |              [crash]
                  |                 |
                  v                 v
               Failed          load_checkpoint()
                  |                 |
               update_status        v
                Running         replay_from()
                                    |
                                    v
                               [resume run]  -->  Done
```

The registry holds a persistent record of every transition. You can build a dashboard, retry
policy, SLO monitor, or human-in-the-loop approval queue on top of it with no additional plumbing.

## Summary

| Operation              | Method                           | What it touches        |
| ---------------------- | -------------------------------- | ---------------------- |
| Create a task          | `reg.submit(kind, input)`        | Persists new Task      |
| Read a task            | `reg.get(&id)`                   | Returns full Task      |
| Advance business state | `reg.update_status(&id, status)` | Task::status field     |
| Save execution trace   | `reg.checkpoint(&id, &crux)`     | Task::checkpoint field |
| Load trace for replay  | `reg.load_checkpoint(&id)`       | Returns `Crux<Value>`  |
| List pending tasks     | `reg.pending(kind)`              | Filters by kind        |
| Snapshot current trace | `ctx.snapshot()`                 | Returns `Crux<Value>`  |
| Seed replay            | `ctx.replay_from(&trace)`        | Populates replay cache |

Chapter **05** covers lifecycle hooks — `on_low_confidence`, `on_step_failure`,
`on_budget_exceeded` — and how to recover gracefully instead of driving straight into the registry
with a `Failed` status.
