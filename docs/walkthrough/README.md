# `crux`: Now available in multiple flavors

`crux` has two surfaces: **YAML pipelines** (`.crux` files) for declarative workflows, and a
**Rust macro API** (`#[crux::agent]`) when you need typed logic, delegation, and
confidence-based routing. Both produce the same `Crux<T>` trace — inspectable, serializable,
and replayable.

If you just want to chain steps, fan out, and call LLMs, write a `.crux` file and run it.
If you need custom control flow, drop into Rust. Rust edition 2024, MSRV 1.88.

## Who this tutorial is for

You already know:

- What an agent loop is (plan → act → observe → repeat)
- YAML is fine for glue; Rust is better when you need types
- Optionally: Rust ownership, `Result`, `?`, `tokio` (needed for chapters 02+)

You want to see:

- How `.crux` pipelines work and what handlers are available
- What `#[crux::agent]` gives you beyond a hand-rolled `tokio::spawn` + channels setup
- How `Crux<T>` differs from `tracing::Span` / OpenTelemetry
- How to build a task planning + execution system with serializable state

## How to read this

The chapters build on each other, but each is self-contained if you know the previous chapter's
types. If you are skimming, read **01** and **05**, then jump to **06** for the hands-on project.

| #   | Chapter                                                            | What you will learn                                                      |
| --- | ------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| 01  | [Setup & Rust toolchain](./01-setup.md)                            | Install, scaffold a project, write your first `Crux`                     |
| 02  | [Core types](./02-core-types.md)                                   | `Crux<T>`, `CruxErr`, `Step`, `Agent` trait                              |
| 03  | [Branching & delegation](./03-branching-delegation.md)             | `match`, confidence routes, `speculate`, `delegate`                      |
| 04  | [Serializable task management](./04-task-registry.md)              | `TaskRegistry`, `Task`, crash-safe replay                                |
| 05  | [Lifecycle hooks](./05-lifecycle-hooks.md)                         | `on_low_confidence`, `on_step_failure`, `on_budget_exceeded`             |
| 06  | [Project: Decomposer + Executor](./06-project-planner-executor.md) | Build a task planning + execution system end-to-end                      |
| 07  | [vs existing agentic patterns](./07-vs-existing-patterns.md)       | How `crux::` compares to LangGraph, CrewAI, `tracing`, hand-rolled loops |
| R   | [Syntax reference card](../crux-syntax-reference.md)               | Every macro, trait, and type in one page                                 |

## The 30-second pitch

A `.crux` pipeline:

```yaml
pipeline: summarize
budget: { calls: 2 }

steps:
  - step: count_words
    handler: shell::capture
    args:
      cmd: "wc -w < input.txt"

  - step: log_result
    handler: ctrl::log
```

The same idea in Rust:

```rust
use crux::prelude::*;

#[crux::agent]
async fn plan_trip(goal: String) -> Crux<Itinerary> {
    let research = x.step("research", || async {
        Ok(search_web(&goal).await?)
    }).await?;

    let draft = x.delegate::<DraftAgent>("draft", research)
        .with_budget(Budget::tokens(4000))
        .run().await?;

    x.speculate("finalize", vec![
        ("cheap", Box::pin(async { finalize_cheap(&draft).await })),
        ("fast",  Box::pin(async { finalize_fast(&draft).await })),
        ("safe",  Box::pin(async { finalize_safe(&draft).await })),
    ]).pick_best_by(|r| r.confidence).await
}
```

The `#[crux::agent]` macro injects a context variable `x` (aliased from `__crux_ctx`) into the
function body. Every `x.step`, `x.delegate`, and `x.speculate` call is recorded in the `Crux<T>`
value the function returns. That value is:

- **Inspectable** — `crux.causal_chain()`, `crux.delegations()`, `crux.rejected_branches()`
- **Serializable** — `serde_json::to_string(&crux)` just works (serde support is always on)
- **Replayable** — `ctx.replay_from(&snapshot)` resumes execution after a crash

Whether you write YAML or Rust, the trace is the same. The rest of this tutorial shows how
each piece works.
