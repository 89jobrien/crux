# 06 — Project: Decomposer + Executor

> Goal: build a full task planning + execution system end-to-end. By the end
> of this chapter you'll have two agents that together take a high-level
> goal, decompose it into a DAG of subtasks, execute them in dependency
> order with concurrency, checkpoint to SQLite, and survive a crash mid-run.

This is the chapter to actually type out. Everything before this was
vocabulary.

## The system we're building

```
      goal: "Ship v2 of the checkout API"
                    |
                    v
         +----------+----------+
         |     Decomposer      |   <- LLM call, returns a DAG of subtasks
         +----------+----------+
                    |
                    v   Vec<Task<ExecStatus>>  in TaskRegistry
                    |
         +----------+----------+
         |     Executor        |   <- walks the DAG, runs tasks concurrently
         +----------+----------+
                    |
                    v
              Report { ... }
```

Two agents. One registry. One CLI that drives them.

## Scaffold

```
trip-planner/
|- Cargo.toml
|- src/
|  |- main.rs           # CLI entry
|  |- types.rs          # Task, Status, Report
|  |- decomposer.rs     # Agent #1
|  |- executor.rs       # Agent #2
|  |- skills/           # Leaf capabilities the executor can call
|  |  |- mod.rs
|  |  |- code.rs
|  |  |- test.rs
|  |  |- docs.rs
```

`Cargo.toml`:

```toml
[dependencies]
crux = { version = "0.1", features = ["serde", "tokio", "sqlite"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }
```

## Step 1: types

`src/types.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type SubtaskId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    pub id: SubtaskId,
    pub title: String,
    pub skill: String,            // "code", "test", "docs"
    pub input: serde_json::Value, // skill-specific
    pub depends_on: Vec<SubtaskId>,
    pub estimate_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub goal: String,
    pub subtasks: Vec<Subtask>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum ExecStatus {
    #[default]
    Queued,
    Blocked { waiting_on: Vec<SubtaskId> },
    Running { started_at: DateTime<Utc> },
    Succeeded { output: serde_json::Value },
    Failed { error: String, attempts: u32 },
    Cancelled,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub goal: String,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub outputs: Vec<(String, serde_json::Value)>,
}
```

Notice: `ExecStatus` is *your* enum. The registry persists it without caring
what the variants are. The `#[default]` attribute is used by
`TaskRegistry::submit` to initialize a new task.

## Step 2: the Decomposer

`src/decomposer.rs`:

```rust
use crux::prelude::*;
use crate::types::{Plan, Subtask};
use anyhow::Result;
use chrono::Utc;
use uuid::Uuid;

#[crux::agent]
pub async fn decompose(goal: String) -> Crux<Plan> {
    // 1. Draft a plan with a cheap model.
    let draft = t.step("draft_plan", || async {
        let raw = call_model(&format!("Plan for: {goal}")).await?;
        parse_plan(&raw)
    }).await?;

    // 2. Critique it.
    let critique = t.delegate::<Critic>("critique", draft.clone())
        .with_budget(Budget::tokens(2000))
        .on_low_confidence(0.7, |score, ctx| async move {
            Recovery::Escalate(Box::pin(
                ctx.delegate::<ExpertCritic>("expert", score).await
            ))
        })
        .await?;

    // 3. If the critic found issues, revise. Otherwise keep the draft.
    let plan = t.route_on_confidence(critique.approval, [
        (0.85.., || async { Ok(draft.clone()) }),
        (0.00..0.85, || async {
            let revised = call_model(&format!(
                "Revise this plan. Goal: {goal}. Draft: {draft:?}. Issues: {}",
                critique.issues.join(", ")
            )).await?;
            parse_plan(&revised)
        }),
    ]).await?;

    // 4. Validate the DAG is acyclic before returning.
    t.step("validate_dag", || async {
        validate_acyclic(&plan.subtasks)?;
        Ok(plan)
    }).await
}

// ... call_model / parse_plan / validate_acyclic elided for brevity ...
```

What's new vs. chapters 01-05:

- **Three different branching primitives in 30 lines.** Plain `t.step` for
  the draft, `delegate` with a call-site hook for the critic, and
  `route_on_confidence` for the revise/keep decision. Each one corresponds
  to a different *kind* of choice — and the crux records each one with a
  different `StepKind`.
- **`validate_dag` is a `t.step`**, not a free function. That's deliberate:
  if someone hands us a cyclic plan, we want the crux to record "validation
  ran and failed" as a first-class step, not as a hidden panic deep in a
  helper.

## Step 3: the Executor

This is the bigger one. The executor walks the DAG, runs leaves
concurrently, and updates the registry as tasks move through states.

`src/executor.rs`:

```rust
use crux::prelude::*;
use crux::registry::{TaskRegistry, TaskId};
use crate::types::{Plan, Subtask, SubtaskId, ExecStatus, Report};
use std::collections::{HashMap, HashSet};
use chrono::Utc;
use anyhow::Result;

#[crux::agent(registry = "reg", checkpoint_every_step)]
pub async fn execute(
    reg: &TaskRegistry,
    plan_task_id: TaskId,
    plan: Plan,
) -> Crux<Report> {
    // 1. Submit every subtask to the registry, hanging them off the parent.
    let ids = t.step("enqueue_subtasks", || async {
        let mut ids = HashMap::new();
        for st in &plan.subtasks {
            let tid = reg.submit_child::<ExecStatus, _>(
                plan_task_id, &st.skill, st
            ).await?;
            ids.insert(st.id, tid);
        }
        Ok(ids)
    }).await?;

    // 2. Topologically sort into execution waves.
    let waves = t.step("plan_waves", || async {
        Ok(topological_waves(&plan.subtasks))
    }).await?;

    // 3. Run each wave concurrently, checkpointing between waves.
    let mut outputs: Vec<(String, serde_json::Value)> = Vec::new();
    let mut failed = 0usize;

    for (i, wave) in waves.into_iter().enumerate() {
        let wave_name = format!("wave_{i}");
        let wave_results = t.step(&wave_name, || async {
            let futures = wave.iter().map(|st| {
                let reg = reg.clone();
                let task_id = ids[&st.id];
                let st = st.clone();
                async move {
                    run_one_subtask(&reg, task_id, st).await
                }
            });
            Ok(futures::future::join_all(futures).await)
        }).await?;

        for (st, result) in wave.iter().zip(wave_results) {
            match result {
                Ok(out) => outputs.push((st.title.clone(), out)),
                Err(e) => {
                    failed += 1;
                    reg.update_status(
                        ids[&st.id],
                        ExecStatus::Failed {
                            error: e.to_string(),
                            attempts: 1,
                        },
                    ).await?;
                }
            }
        }
    }

    Ok(Report {
        goal: plan.goal.clone(),
        total: plan.subtasks.len(),
        succeeded: outputs.len(),
        failed,
        outputs,
    })
}

async fn run_one_subtask(
    reg: &TaskRegistry,
    task_id: TaskId,
    subtask: Subtask,
) -> Result<serde_json::Value, CruxErr> {
    reg.update_status(task_id, ExecStatus::Running {
        started_at: Utc::now(),
    }).await?;

    let output = match subtask.skill.as_str() {
        "code" => crate::skills::code::run(subtask.input).await?,
        "test" => crate::skills::test::run(subtask.input).await?,
        "docs" => crate::skills::docs::run(subtask.input).await?,
        other  => return Err(CruxErr::step_failed(
            "dispatch", format!("unknown skill: {other}"),
        )),
    };

    reg.update_status(task_id, ExecStatus::Succeeded {
        output: output.clone(),
    }).await?;

    Ok(output)
}

fn topological_waves(subtasks: &[Subtask]) -> Vec<Vec<Subtask>> {
    let mut done: HashSet<SubtaskId> = HashSet::new();
    let mut waves: Vec<Vec<Subtask>> = Vec::new();
    let mut remaining: Vec<Subtask> = subtasks.to_vec();

    while !remaining.is_empty() {
        let (ready, rest): (Vec<_>, Vec<_>) = remaining
            .into_iter()
            .partition(|st| st.depends_on.iter().all(|d| done.contains(d)));
        if ready.is_empty() {
            panic!("cycle detected in DAG");
        }
        for st in &ready {
            done.insert(st.id);
        }
        waves.push(ready);
        remaining = rest;
    }
    waves
}
```

Walking through this:

### `submit_child`

`reg.submit_child::<S, I>(parent_id, kind, input)` creates a task that
inherits the parent's `TaskId` in its `parent` field. When you query the
registry, you can fetch a parent and recursively walk its children to get
the full tree.

### Waves instead of continuous scheduling

We run the DAG in waves — one wave per topological level — rather than
continuously draining a ready-queue. Three reasons:

1. **The crux stays readable.** Each wave is one `t.step`, so the final
   crux has a flat sequence of wave steps instead of an interleaved mess.
2. **Checkpointing happens at wave boundaries.** If the process crashes
   during wave 3, replay restarts at wave 3 — not at wave 1.
3. **Concurrency is obvious.** Anyone reading the crux can see "wave 2 had
   4 subtasks running in parallel" without parsing span timings.

If you need continuous scheduling, you can build it — but waves are almost
always enough, and they're easier to reason about.

### `futures::future::join_all` instead of `Crux::join_all`

This is a subtle one. We're using raw `join_all` here because the
per-subtask futures are *not* `#[crux::agent]` functions — they're helper
functions that return `Result<_, CruxErr>`. If we wanted each subtask to
get its own sub-crux attached to the parent, we'd convert `run_one_subtask`
into a `#[crux::agent]` and use `Crux::join_all(...)` instead. Both work;
the agent version gives richer cruxs at the cost of slightly more
ceremony.

### Where does replay pick up?

Because the macro injects `checkpoint_every_step` and we've broken the
execution into explicit `t.step` waves, replay after a crash does this:

1. Load the most recent checkpoint for `execute`.
2. Re-run `execute` from the top.
3. The `enqueue_subtasks` step's input hash matches the original, so it's
   skipped.
4. The `plan_waves` step's input hash also matches, so it's skipped.
5. For each `wave_N` step, if the recorded output exists, skip. Otherwise,
   run the wave fresh.
6. The first wave that doesn't have a recorded output is where work
   actually resumes.

You get wave-level replay for free, because you structured the executor
around `t.step` waves. That's the "crash-safe without writing checkpoint
code" payoff.

## Step 4: the CLI

`src/main.rs`:

```rust
use clap::{Parser, Subcommand};
use crux::registry::TaskRegistry;
use std::path::PathBuf;

mod types;
mod decomposer;
mod executor;
mod skills;

#[derive(Parser)]
#[command(name = "planner")]
struct Cli {
    #[arg(long, default_value = "tasks.db")]
    db: PathBuf,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Plan + execute a goal end-to-end.
    Run { goal: String },
    /// Resume any tasks left pending by a prior crash.
    Resume,
    /// Show the latest crux as pretty JSON.
    Show { task_id: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let reg = TaskRegistry::sqlite(&cli.db)?;

    match cli.cmd {
        Cmd::Run { goal } => {
            let plan_crux = decomposer::decompose(goal.clone()).await;
            let plan = plan_crux.value()?;

            let plan_id = reg.submit::<types::ExecStatus, _>("plan", &plan).await?;
            let exec_crux = executor::execute(&reg, plan_id, plan).await;
            let report = exec_crux.value()?;

            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Cmd::Resume => {
            let pending = reg.pending::<types::ExecStatus>().await?;
            for task in pending {
                println!("resuming {}...", task.id);
                let _ = reg.resume::<types::ExecStatus, executor::ExecuteAgent>(task.id).await?;
            }
        }
        Cmd::Show { task_id } => {
            let id = task_id.parse()?;
            let task = reg.get::<types::ExecStatus>(id).await?;
            println!("{}", serde_json::to_string_pretty(&task)?);
        }
    }
    Ok(())
}
```

## Exercises

If you want to push the tutorial further, try one of these. Each uses a
different set of `crux::` features.

| # | Exercise | Which features |
|---|----------|----------------|
| 1 | Add an `on_budget_exceeded` hook that downgrades `PolishedReport` to `TemplateReport` when tokens run low | Lifecycle hooks + delegation |
| 2 | Add a `speculate` call in the decomposer that runs three draft models and picks the best | Speculation + rejected branches |
| 3 | Implement `skills::review` as a human-in-the-loop task that leaves the task in `AwaitingApproval` until marked done via a CLI command | Status machines + registry |
| 4 | Add a `--replay` flag to the CLI that loads a saved task and re-runs it, asserting the crux is identical | Replay + input hashes |
| 5 | Switch from waves to continuous scheduling, building a `reg.ready_to_run::<ExecStatus>()` query on top of the registry | Registry internals |

## The payoff

Count the lines of infrastructure code in the whole project:

- 0 lines of manual span code
- 0 lines of retry loops (hooks do it)
- 0 lines of checkpoint code (`checkpoint_every_step` does it)
- 0 lines of replay logic (`reg.resume` does it)
- ~30 lines of topological-sort logic (because that's actually your domain)

Everything that isn't your domain is language-provided. That's the design
goal.

## Check your understanding

- **Why run waves instead of continuous scheduling?** *Cleaner cruxs,
  checkpoint alignment, obvious concurrency.*
- **What happens if you crash mid-wave?** *On resume, `enqueue_subtasks`
  and `plan_waves` are replayed from cache; previous completed waves are
  replayed from cache; the crashed wave re-runs.*
- **How does the Decomposer use three different branching primitives?**
  *`t.step` for the draft, `delegate` + hook for the critic,
  `route_on_confidence` for revise-or-keep.*
- **Why isn't `run_one_subtask` a `#[crux::agent]`?** *It's a helper, and
  we wanted raw `join_all`. You could absolutely convert it and use
  `Crux::join_all` for richer sub-cruxs.*

Chapter **07** is the comparison against existing agentic patterns — the
one you originally asked for.
