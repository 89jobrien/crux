# `cruxx::` — an agentic DSL for Rust

`cruxx::` is not a standalone language — it is a Rust DSL (a set of macros, traits, and types)
that makes agentic control flow explicit in the type system. Every step, delegation, speculation,
and failure is a first-class value (`Crux<T>`) that is inspectable, serializable, and replayable.

If you have written Rust agents with `tokio` + `tracing` + a hand-rolled task queue, `cruxx::` is
what happens when you bake those patterns into the language itself. Rust edition 2024, MSRV 1.85.

## Who this tutorial is for

You already know:

- Rust ownership, `Result`, `?`, trait objects, `tokio`
- What an agent loop is (plan → act → observe → repeat)
- Why you need confidence scores, delegation, and replay

You want to see:

- What `cruxx::` gives you that a hand-rolled `tokio::spawn` + channels setup does not
- How `Crux<T>` differs from `tracing::Span` / OpenTelemetry
- How to build a task planning + execution system with serializable state
- Where the language draws the line between "language feature" and "library"

## How to read this

The chapters build on each other, but each is self-contained if you know the previous chapter's
types. If you are skimming, read **01** and **05**, then jump to **06** for the hands-on project.

| #  | Chapter                                                               | What you will learn                                                       |
|----|-----------------------------------------------------------------------|---------------------------------------------------------------------------|
| 01 | [Setup & Rust toolchain](./01-setup.md)                               | Install, scaffold a project, write your first `Crux`                      |
| 02 | [Core types](./02-core-types.md)                                      | `Crux<T>`, `CruxErr`, `Step`, `Agent` trait                               |
| 03 | [Branching & delegation](./03-branching-delegation.md)                | `match`, confidence routes, `speculate`, `delegate`                       |
| 04 | [Serializable task management](./04-task-registry.md)                 | `TaskRegistry`, `Task`, crash-safe replay                              |
| 05 | [Lifecycle hooks](./05-lifecycle-hooks.md)                            | `on_low_confidence`, `on_step_failure`, `on_budget_exceeded`              |
| 06 | [Project: Decomposer + Executor](./06-project-planner-executor.md)    | Build a task planning + execution system end-to-end                       |
| 07 | [vs existing agentic patterns](./07-vs-existing-patterns.md)          | How `cruxx::` compares to LangGraph, CrewAI, `tracing`, hand-rolled loops |
| R  | [Syntax reference card](../crux-syntax-reference.md)                  | Every macro, trait, and type in one page                                  |

## The 30-second pitch

```rust
use cruxx::prelude::*;

#[cruxx::agent]
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

The `#[cruxx::agent]` macro injects a context variable `x` (aliased from `__cruxx_ctx`) into the
function body. Every `x.step`, `x.delegate`, and `x.speculate` call is recorded in the `Crux<T>`
value the function returns. That value is:

- **Inspectable** — `cruxx.causal_chain()`, `cruxx.delegations()`, `cruxx.rejected_branches()`
- **Serializable** — `serde_json::to_string(&cruxx)` just works (serde support is always on)
- **Replayable** — `ctx.replay_from(&snapshot)` resumes execution after a crash

That is the whole language. The rest of this tutorial shows how each piece works and where it
differs from what you would already build by hand.
