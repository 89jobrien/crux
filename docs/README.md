# `trace::` — an agentic DSL for Rust

> A small, Rust-embedded DSL for writing agentic programs where every step,
> branch, delegation, and failure is a first-class value you can inspect,
> replay, and serialize.

`trace::` is not a standalone language — it's a Rust DSL (a set of macros,
traits, and types) that makes agentic control flow explicit in the type
system. If you've written Rust agents with `tokio` + `tracing` + a hand-rolled
task queue, `trace::` is what happens when you bake those patterns into the
language itself.

## Who this tutorial is for

You already know:

- Rust ownership, `Result`, `?`, trait objects, `tokio`
- What an agent loop is (plan → act → observe → repeat)
- Why you need confidence scores, delegation, and replay

You want to see:

- What `trace::` gives you that a hand-rolled `tokio::spawn` + channels setup
  doesn't
- How `Trace<T>` differs from `tracing::Span` / OpenTelemetry
- How to build a task planning + execution system with serializable state
- Where the language draws the line between "language feature" and "library"

## How to read this

The chapters build on each other, but each one is self-contained if you know
the previous chapter's types. If you're skimming, read **01** and **05**,
then jump to **06** for the hands-on project.

| # | Chapter | What you'll learn |
|---|---------|------------------|
| 01 | [Setup & Rust toolchain](./01-setup.md) | Install, scaffold a project, write your first `Trace` |
| 02 | [Core types](./02-core-types.md) | `Trace<T>`, `TraceErr`, `Step`, `Agent` trait |
| 03 | [Branching & delegation](./03-branching-delegation.md) | `match`, confidence routes, `speculate`, `delegate` |
| 04 | [Serializable task management](./04-task-registry.md) | `TaskRegistry`, `Task<S>`, crash-safe replay |
| 05 | [Lifecycle hooks](./05-lifecycle-hooks.md) | `on_low_confidence`, `on_step_failure`, `on_budget_exceeded` |
| 06 | [Project: Decomposer + Executor](./06-project-planner-executor.md) | Build a task planning + execution system end-to-end |
| 07 | [vs existing agentic patterns](./07-vs-existing-patterns.md) | How `trace::` compares to LangGraph, CrewAI, `tracing`, hand-rolled loops |
| R  | [Syntax reference card](./reference.md) | Every macro, trait, and type in one page |

## The 30-second pitch

```rust
use trace::prelude::*;

#[trace::agent]
async fn plan_trip(goal: String) -> Trace<Itinerary> {
    let research = t.step("research", || search_web(&goal)).await?;

    let draft = t.delegate::<DraftAgent>("draft", &research)
        .with_budget(Budget::tokens(4000))
        .on_low_confidence(0.7, escalate_to_human)
        .await?;

    t.speculate("finalize", [
        ("cheap", || finalize_cheap(&draft)),
        ("fast",  || finalize_fast(&draft)),
        ("safe",  || finalize_safe(&draft)),
    ]).pick_best_by(|r| r.confidence).await
}
```

Every `t.step`, `t.delegate`, `t.speculate` call is recorded in the `Trace<T>`
value the function returns. That value is:

- **Inspectable** — `trace.causal_chain()`, `trace.delegations()`, `trace.rejected_branches()`
- **Serializable** — `serde_json::to_string(&trace)` just works
- **Replayable** — `Trace::replay_from(snapshot)` resumes after a crash
- **Composable** — `trace_a | trace_b`, `Trace::join_all([...])`

That's the whole language. The rest of this tutorial is just showing you how
each piece works and where it differs from what you'd already build by hand.
