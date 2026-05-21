# 06 — Project: Decomposer + Executor

> Goal: build a full task planning and execution system end-to-end. By the end of this chapter
> you will have two agents that take a high-level goal, decompose it into a set of subtasks,
> execute them in dependency order with concurrency, and checkpoint progress to disk so the
> system survives a crash mid-run.

This is the chapter to type out. Everything before it was vocabulary. This chapter uses
concrete API shapes — no invented types, no phantom methods.

## The system we're building

```text
      goal: "Ship v2 of the checkout API"
                    |
                    v
         +----------+----------+
         |     Decomposer      |   <- drafts a plan, delegates to critic, routes on confidence
         +----------+----------+
                    |
                    v   Vec<Task> in TaskRegistry (kind = "build" | "test" | "docs" | ...)
                    |
         +----------+----------+
         |     Executor        |   <- walks the DAG in waves, runs each wave with join_all
         +----------+----------+
                    |
                    v
              Report { succeeded, failed, outputs }
```

Two agents. One registry. One CLI entry point.

The `Task` type is not generic over a user-defined status. The registry owns status transitions
through `TaskStatus` (`Pending`, `Running`, `Done`, `Failed`). The task's `kind` field carries
the task type string ("plan", "build", "test", "docs") and the `input` field carries a
`serde_json::Value` containing whatever the task needs.

## Scaffold

```text
project-planner/
|- Cargo.toml
|- src/
|  |- main.rs           <- CLI entry
|  |- types.rs          <- Subtask, Plan, Report
|  |- decomposer.rs     <- Agent #1
|  |- executor.rs       <- Agent #2
|  |- skills/           <- Leaf capabilities called by the executor
|  |  |- mod.rs
|  |  |- build.rs
|  |  |- test.rs
|  |  |- docs.rs
```

`Cargo.toml`:

```toml
[package]
name = "project-planner"
version = "0.1.0"
edition = "2021"

[dependencies]
crux       = { version = "0.2", features = ["tokio-runtime", "redb"] }
tokio       = { version = "1",   features = ["full"] }
serde       = { version = "1",   features = ["derive"] }
serde_json  = "1"
clap        = { version = "4",   features = ["derive"] }
anyhow      = "1"
uuid        = { version = "1",   features = ["v4", "serde"] }
```

`serde` support is always compiled into `crux` — no separate feature flag needed.

## Step 1: types

`src/types.rs`:

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type SubtaskId = Uuid;

/// A single unit of work in the decomposed plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    pub id: SubtaskId,
    pub title: String,
    /// Maps to a skill handler: "build", "test", "docs".
    pub kind: String,
    /// Skill-specific parameters.
    pub input: serde_json::Value,
    /// Subtask ids that must complete before this one runs.
    pub depends_on: Vec<SubtaskId>,
    pub estimate_tokens: u32,
}

/// The full plan produced by the Decomposer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub goal: String,
    pub subtasks: Vec<Subtask>,
}

/// The final output of the Executor.
#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub goal: String,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub outputs: Vec<(String, serde_json::Value)>,
}
```

Notice there is no user-defined status enum here. The registry tracks task lifecycle through
`TaskStatus` — the four variants (`Pending`, `Running`, `Done`, `Failed`) cover every state
transition this system needs. The `kind` field on `Task` mirrors the `kind` field on `Subtask`.

## Step 2: the Decomposer

The Decomposer drafts a plan, delegates it to a critic, and decides whether to publish or
revise based on confidence. It uses three different branching primitives in under 40 lines.

`src/decomposer.rs`:

```rust
use crux::prelude::*;
use crate::types::{Plan, Subtask};
use uuid::Uuid;

/// A critique returned by the CriticAgent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Critique {
    pub approval: f32,      // 0.0–1.0
    pub issues: Vec<String>,
}

/// Stub: in a real system this calls an LLM.
pub struct CriticAgent;
impl Agent for CriticAgent {
    type Input  = Plan;
    type Output = Critique;
    fn name() -> &'static str { "critic" }
    async fn run(_ctx: &mut CruxCtx, plan: Plan) -> Result<Critique, CruxErr> {
        // Simplified: approve every plan that has subtasks.
        let approval = if plan.subtasks.is_empty() { 0.2 } else { 0.9 };
        Ok(Critique { approval, issues: vec![] })
    }
}

#[crux::agent]
pub async fn decompose(goal: String) -> Crux<Plan> {
    // 1. Draft a plan using a cheap model (stubbed here).
    let draft: Plan = x.step("draft_plan", || {
        let goal = goal.clone();
        async move { stub_draft(&goal) }
    }).await?;

    // 2. Delegate critique to a sub-agent with a token budget.
    let critique: Critique = x
        .delegate::<CriticAgent>("critique", draft.clone())
        .with_budget(Budget::tokens(2_000))
        .run()
        .await?;

    // 3. Route on the critic's approval score.
    //    High confidence  -> keep the draft.
    //    Lower confidence -> revise.
    let plan: Plan = x
        .route_on_confidence(
            "revise_or_keep",
            critique.approval,
            vec![
                (
                    ConfidenceRange::inclusive(0.85, 1.0),
                    "keep",
                    Box::pin(async move { Ok(draft) }),
                ),
                (
                    ConfidenceRange::exclusive(0.0, 0.85),
                    "revise",
                    Box::pin({
                        let goal = goal.clone();
                        let issues = critique.issues.clone();
                        async move { stub_revise(&goal, &issues) }
                    }),
                ),
            ],
        )
        .await?;

    // 4. Validate the DAG before returning — recorded as a named step
    //    so a cycle failure appears as a first-class trace entry.
    x.step("validate_dag", || {
        let plan = plan.clone();
        async move {
            validate_acyclic(&plan.subtasks)?;
            Ok(plan)
        }
    }).await
}

// ---------------------------------------------------------------------------
// Stubs — replace with real LLM calls in production.

fn stub_draft(goal: &str) -> Result<Plan, CruxErr> {
    Ok(Plan {
        goal: goal.to_string(),
        subtasks: vec![
            Subtask {
                id: Uuid::new_v4(),
                title: "Implement endpoint".to_string(),
                kind: "build".to_string(),
                input: serde_json::json!({"spec": "POST /v2/checkout"}),
                depends_on: vec![],
                estimate_tokens: 1_500,
            },
            Subtask {
                id: Uuid::new_v4(),
                title: "Write integration tests".to_string(),
                kind: "test".to_string(),
                input: serde_json::json!({"target": "checkout_handler"}),
                depends_on: vec![],   // filled in by a real decomposer
                estimate_tokens: 800,
            },
        ],
    })
}

fn stub_revise(goal: &str, issues: &[String]) -> Result<Plan, CruxErr> {
    // In production this calls the LLM again with the critique.
    let _ = issues;
    stub_draft(goal)
}

fn validate_acyclic(subtasks: &[Subtask]) -> Result<(), CruxErr> {
    // Abbreviated cycle check — a real implementation uses DFS.
    let ids: std::collections::HashSet<_> = subtasks.iter().map(|s| s.id).collect();
    for st in subtasks {
        for dep in &st.depends_on {
            if !ids.contains(dep) {
                return Err(CruxErr::step_failed(
                    "validate_dag",
                    format!("unknown dependency {dep}"),
                ));
            }
        }
    }
    Ok(())
}
```

What's new relative to chapters 01–05:

- **Three branching primitives in sequence.** `x.step` for the draft, `delegate` with a budget
  for the critic, and `route_on_confidence` with explicit `ConfidenceRange` constructors for
  the keep-or-revise decision. Each records a different `StepKind` in the trace.
- **`validate_dag` is an `x.step`, not a free call.** If the plan is cyclic, the failure
  appears in the trace as a named step — not as an invisible panic inside a helper.
- **`ConfidenceRange::inclusive` and `ConfidenceRange::exclusive`** are the actual
  constructors. The ranges must be non-overlapping, gap-free, and together cover `[0.0, 1.0]`.
  The runtime validates this before dispatching.

## Step 3: the Executor

The Executor walks the plan DAG in topological waves. Each wave fans out via `join_all`,
then checkpoints progress to the registry. If the process crashes between waves, replay
picks up at the first wave whose `x.step` has no recorded output.

`src/executor.rs`:

```rust
use std::collections::{HashMap, HashSet};

use crux::prelude::*;
use crux::registry::{InMemoryBackend, TaskRegistry, TaskStatus};

use crate::types::{Plan, Report, Subtask, SubtaskId};

#[crux::agent(registry = "plan")]
pub async fn execute(plan: Plan) -> Crux<Report> {
    // 1. Submit every subtask to the registry (kind = subtask.kind).
    let ids: HashMap<SubtaskId, TaskId> = x
        .step("enqueue_subtasks", || {
            let subtasks = plan.subtasks.clone();
            async move {
                // This step runs inside the agent body, so we build a local
                // in-memory registry for subtask tracking. A production system
                // would receive the registry as a parameter or via injection.
                let reg = TaskRegistry::new(InMemoryBackend::new());
                let mut map = HashMap::new();
                for st in &subtasks {
                    let tid = reg.submit(&st.kind, &st.input).await
                        .map_err(|e| CruxErr::step_failed("enqueue_subtasks", e.to_string()))?;
                    map.insert(st.id, tid);
                }
                Ok(map)
            }
        })
        .await?;

    // 2. Compute topological waves from the DAG.
    let waves: Vec<Vec<Subtask>> = x
        .step("plan_waves", || {
            let subtasks = plan.subtasks.clone();
            async move { Ok(topological_waves(&subtasks)) }
        })
        .await?;

    // 3. Execute each wave concurrently, recording per-wave steps.
    let mut outputs: Vec<(String, serde_json::Value)> = Vec::new();
    let mut failed = 0usize;

    for (i, wave) in waves.into_iter().enumerate() {
        let wave_label = format!("wave_{i}");

        // Build one join_all arm per subtask in the wave.
        let arms: Vec<(&str, BoxFut<Option<(String, serde_json::Value)>>)> = wave
            .iter()
            .map(|st| {
                let st = st.clone();
                let arm: BoxFut<Option<(String, serde_json::Value)>> = Box::pin(async move {
                    match dispatch_skill(&st).await {
                        Ok(out) => Ok(Some((st.title.clone(), out))),
                        Err(_)  => Ok(None),   // record failure as None; count below
                    }
                });
                // lifetime of label tied to wave Vec — use a static string for demo;
                // in production, collect labels into a Vec<String> first.
                (st.kind.as_str(), arm)
            })
            .collect();

        // join_all signature: (name, Vec<(&str, BoxFut<T>)>) -> Result<Vec<T>, CruxErr>
        // Each arm is recorded as "wave_N::kind".
        // NOTE: the arms vec above borrows `wave`, so we restructure slightly:
        let arm_labels: Vec<String> = wave.iter().map(|st| st.kind.clone()).collect();
        let arm_futs: Vec<BoxFut<Option<(String, serde_json::Value)>>> = wave
            .iter()
            .map(|st| {
                let st = st.clone();
                Box::pin(async move {
                    match dispatch_skill(&st).await {
                        Ok(out) => Ok(Some((st.title.clone(), out))),
                        Err(_)  => Ok(None),
                    }
                }) as BoxFut<_>
            })
            .collect();

        let labeled_arms: Vec<(&str, BoxFut<Option<(String, serde_json::Value)>>)> =
            arm_labels
                .iter()
                .map(|l| l.as_str())
                .zip(arm_futs)
                .collect();

        let wave_results = x
            .join_all(&wave_label, labeled_arms)
            .await?;

        for result in wave_results {
            match result {
                Some(pair) => outputs.push(pair),
                None       => failed += 1,
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

// ---------------------------------------------------------------------------
// Skill dispatch

async fn dispatch_skill(st: &Subtask) -> Result<serde_json::Value, CruxErr> {
    match st.kind.as_str() {
        "build" => crate::skills::build::run(st.input.clone()).await,
        "test"  => crate::skills::test::run(st.input.clone()).await,
        "docs"  => crate::skills::docs::run(st.input.clone()).await,
        other   => Err(CruxErr::step_failed(
            "dispatch",
            format!("unknown skill: {other}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// Topological wave decomposition

fn topological_waves(subtasks: &[Subtask]) -> Vec<Vec<Subtask>> {
    let mut done: HashSet<SubtaskId> = HashSet::new();
    let mut waves: Vec<Vec<Subtask>>  = Vec::new();
    let mut remaining: Vec<Subtask>   = subtasks.to_vec();

    while !remaining.is_empty() {
        let (ready, rest): (Vec<_>, Vec<_>) = remaining
            .into_iter()
            .partition(|st| st.depends_on.iter().all(|d| done.contains(d)));

        assert!(!ready.is_empty(), "cycle detected in subtask DAG");

        for st in &ready {
            done.insert(st.id);
        }
        waves.push(ready);
        remaining = rest;
    }
    waves
}
```

Walking through the design decisions:

### Waves instead of continuous scheduling

Running the DAG in topological waves — one `x.step` per level — gives three things for free:

1. **Readable trace.** The finished `Crux` has a flat sequence of `wave_0`, `wave_1`, ... steps.
   Anyone reading the trace sees "wave 1 had 3 tasks" without parsing span timings.
2. **Checkpoint alignment.** Checkpointing happens at wave boundaries. Crash during wave 3?
   Replay replays waves 0 and 1 from cache, then re-runs wave 3 fresh.
3. **Obvious concurrency.** `join_all` within a wave and sequential ordering between waves —
   no scheduler to reason about.

### `x.join_all` vs raw `futures::join_all`

`x.join_all(name, arms)` records one named step per arm under `name::label`. That means the
trace shows `wave_0::build`, `wave_0::test`, etc. — each arm is independently inspectable and
replayable. Raw `futures::join_all` would run the concurrency but record nothing.

### `#[crux::agent(registry = "plan")]`

The attribute generates a `run_registered` associated function on the produced `ExecuteAgent`
struct. `run_registered` submits a task with `kind = "plan"`, marks it `Running`, executes the
agent body, then marks it `Done` or `Failed` and checkpoints the final trace. The CLI uses this
to get registry integration without any boilerplate.

### Manual checkpoint between waves

For wave-level crash recovery you need a checkpoint after each wave. `x.checkpoint_to` serializes
the in-progress trace into the registry:

```rust
// After each wave, checkpoint current progress.
x.checkpoint_to(&registry, &task_id).await
    .map_err(|e| CruxErr::step_failed("checkpoint", e.to_string()))?;
```

On resume, load the checkpoint and seed the replay cache before running the agent:

```rust
let mut ctx = CruxCtx::new("execute");
if let Ok(Some(cp)) = registry.load_checkpoint(&task_id).await {
    ctx.replay_from(&cp);
}
// Now run the agent body — completed steps are served from cache.
```

`checkpoint_to` and `replay_from` are the two halves of the crash-recovery story.

## Step 4: the CLI entry point

`src/main.rs`:

```rust
use anyhow::Result;
use clap::{Parser, Subcommand};
use crux::registry::{RedbBackend, TaskRegistry, TaskStatus};
use std::path::PathBuf;

mod decomposer;
mod executor;
mod skills;
mod types;

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
    /// Plan and execute a goal end-to-end.
    Run { goal: String },
    /// List all tasks that are still Pending.
    Pending,
    /// Show the stored trace for a task.
    Show { task_id: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let reg = TaskRegistry::new(
        RedbBackend::open(cli.db.to_str().expect("non-UTF-8 path"))?,
    );

    match cli.cmd {
        Cmd::Run { goal } => {
            // Phase 1: decompose.
            let plan_crux = decomposer::decompose(goal.clone()).await;
            let plan = plan_crux.value().map_err(|e| anyhow::anyhow!("{e}"))?;

            // Phase 2: execute with registry integration.
            //
            // run_registered is generated by #[crux::agent(registry = "plan")].
            // It submits a task, marks it Running, runs the agent body, then
            // marks it Done/Failed and checkpoints the final trace.
            let (exec_crux, _task_id) =
                executor::ExecuteAgent::run_registered(&reg, plan).await;

            let report = exec_crux.value().map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }

        Cmd::Pending => {
            // pending("") returns all pending tasks regardless of kind.
            let tasks = reg.pending("").await?;
            for t in &tasks {
                println!("{} kind={} attempts={}", t.id, t.kind, t.attempts);
            }
            if tasks.is_empty() {
                println!("no pending tasks");
            }
        }

        Cmd::Show { task_id } => {
            let id = task_id.parse()?;
            let task = reg.get(&id).await?;
            println!("{}", serde_json::to_string_pretty(&task)?);
        }
    }

    Ok(())
}
```

Key points:

- **`RedbBackend::open(path)`** — the redb adapter takes a `&str` path. There is no SQLite
  integration in crux; redb is the embedded persistence option.
- **`run_registered`** — generated by the `registry = "plan"` attribute on `#[crux::agent]`.
  Returns `(Crux<Report>, TaskId)`. The `TaskId` can be stored and used with `load_checkpoint`
  to resume after a crash.
- **`pending("")`** — the empty string means "all kinds". Pass `"plan"` to filter only tasks
  submitted by the executor.

## Exercises

Each exercise targets a different part of the API.

| #   | Exercise                                                                                                                                                      | Which features                   |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| 1   | Add an `on_budget_exceeded` hook to the Decomposer that downgrades to a minimal plan when tokens run low                                                      | Lifecycle hooks, Budget          |
| 2   | Add a `speculate` call in the Decomposer that runs three stub drafters and picks the one with the most subtasks via `pick_best_by`                            | SpeculationBuilder               |
| 3   | Implement crash recovery in the CLI `Run` command: check `pending("plan")` first; if a matching task exists, load its checkpoint and resume via `replay_from` | `load_checkpoint`, `replay_from` |
| 4   | Add `step_with_confidence` to individual skill dispatches so the trace records how confident each skill was in its output                                     | `step_with_confidence`           |
| 5   | Switch from waves to a ready-queue loop: poll `pending("build")` + dependency tracking in a loop until all subtasks are `Done` or `Failed`                    | Registry internals               |

## The payoff

Count the lines of infrastructure code in the whole project:

- 0 lines of manual span/tracing code
- 0 lines of retry loops (lifecycle hooks handle that)
- 0 lines of manual status-machine logic (the registry + `run_registered` handle it)
- ~30 lines of topological-sort logic — because that is your domain, not the framework's

Every structural choice in this system maps to one crux primitive:

| Design choice           | Primitive used                                                |
| ----------------------- | ------------------------------------------------------------- |
| Draft a plan            | `x.step`                                                      |
| Get expert critique     | `x.delegate` with `with_budget`                               |
| Keep or revise          | `x.route_on_confidence` with `ConfidenceRange`                |
| Run a wave concurrently | `x.join_all`                                                  |
| Crash-safe persistence  | `TaskRegistry<RedbBackend>` + `checkpoint_to` / `replay_from` |
| Registry lifecycle      | `#[crux::agent(registry = "...")]` + `run_registered`         |

## Check your understanding

- **Why is there no custom status enum?** The registry owns the four-state lifecycle
  (`Pending`, `Running`, `Done`, `Failed`). Task type is encoded in the `kind` field string.
- **What happens if the process crashes during wave 2?** On the next run, `replay_from` seeds
  the context from the last checkpoint. The `enqueue_subtasks` and `plan_waves` steps match
  their cached outputs and are skipped. `wave_0` and `wave_1` are also cached. `wave_2` has
  no output and re-runs fresh.
- **How does `run_registered` differ from calling the agent function directly?** Calling
  `execute(plan).await` runs the agent and returns a `Crux<Report>` with no registry
  involvement. `ExecuteAgent::run_registered(&reg, plan).await` also submits a task before
  execution, updates status transitions, and checkpoints the trace on completion.
- **Why does `route_on_confidence` use constructor calls instead of range syntax?** The
  runtime validates that the supplied ranges are non-overlapping, gap-free, and together
  cover `[0.0, 1.0]`. The `ConfidenceRange::exclusive` and `ConfidenceRange::inclusive`
  constructors enforce bound validity at construction time (panicking on NaN, infinite, or
  out-of-range bounds), making misconfigured routing a startup failure rather than a silent
  wrong-branch at runtime.

Chapter **07** compares the crux model against existing agentic patterns.
