# 07 — `trace::` vs existing agentic patterns

> Goal: place `trace::` on the map. If you've built agentic systems before,
> this chapter tells you which patterns `trace::` borrows, which it
> replaces, and which it deliberately leaves alone.

This is the chapter you asked for originally. Each comparison is side-by-
side: the pattern you'd already know, and what changes under `trace::`.

## vs. hand-rolled `tokio::spawn` + channels

**The hand-rolled version** — every Rust agent author has written this:

```rust
let (tx, mut rx) = mpsc::channel(32);

tokio::spawn(async move {
    let result = plan(&goal).await;
    tx.send(PlanEvent::Done(result)).await.ok();
});

while let Some(event) = rx.recv().await {
    match event {
        PlanEvent::Done(r) => { ... }
        PlanEvent::Failed(e) => { ... retry ... }
    }
}
```

You end up with:

- A bespoke `PlanEvent` enum per agent
- Retry logic at the `rx.recv()` site
- Tracing via `tracing::info!` scattered through the code
- No record of what the agent *considered* — only what it did

**Under `trace::`:**

```rust
let trace = plan(goal).await;
if let Err(e) = trace.value() {
    // retry handled by hook; this path is just propagation
}
```

What moved:

| Hand-rolled | `trace::` |
|-------------|-----------|
| Event enum per agent | `Trace<T>` is the universal event type |
| Retry logic at receiver | `on_step_failure` hook at the agent |
| Tracing via `info!` | Every `t.step` is already a recorded event |
| No record of rejected options | `speculate` + `rejected_branches()` |

The tradeoff is that `trace::` enforces its shape — you *must* structure
work as steps, delegations, and speculations. If your agent looks like
"one big LLM call and then save the result", `trace::` is overkill and
you should just use `tokio::spawn`.

## vs. the `tracing` / `tracing-opentelemetry` crate

This is the closest cousin. `tracing` gives you `Span`, `Event`, and the
macro infrastructure to record them. It's the de facto standard in Rust
observability.

**Where they overlap:**

- Both have a concept of structured, hierarchical records
- Both have automatic propagation via task-local context
- Both support JSON serialization of the record stream

**Where they diverge:**

| `tracing` | `trace::` |
|-----------|-----------|
| Spans are observability | Steps are program state |
| `Span` is a side effect | `Trace<T>` is the return value |
| Events are fire-and-forget | Steps hold their output |
| No concept of confidence | `confidence: f32` is a first-class field |
| No concept of rejected branches | `speculate` records losers |
| Replay is not a goal | Replay is a language feature |

The one-sentence version: **`tracing` tells you what happened; `trace::`
tells you what happened *and* lets you re-run it.**

Concretely, you can't replay a `tracing` span back through your code
because the span doesn't contain the inputs to the functions that ran
under it. `trace::` stores the input hash of each step, so replay is
correct-by-construction.

**When to use which:**

- Stick with `tracing` for RPC services, web servers, CI systems — places
  where you want observability but not replay.
- Use `trace::` for agents — places where the *decision process* is
  itself the interesting artifact.
- You can use both in the same process. `trace::` emits `tracing` events
  under the hood if the `tracing` feature flag is on, so your existing
  APM dashboards keep working.

## vs. your own `agent_trace` crate

If you've built something like `agent_trace` yourself, you've probably
reinvented most of `trace::`. The patterns converge because the problems
are the same. The differences tend to be:

| Your hand-rolled version | `trace::` |
|--------------------------|-----------|
| `AgentRun` struct in your code | `Trace<T>` as a generic type |
| Manual `run.record_step(...)` calls | `t.step(...)` via macro |
| Checkpointing bolted on later | `#[trace::agent(checkpoint_every_step)]` |
| `TracedProvider` trait for LLM calls | `Agent` trait for any delegatable work |
| Status enum per agent | `Task<S>` with user-defined `S` |

The most interesting question is: **should you throw out your own crate
and use `trace::`?** The honest answer is that it depends on three things:

1. **How much of your own crate is domain-specific?** The `TracedProvider`
   pattern — wrapping LLM providers with trace-recording middleware — is
   something you probably want to keep. `trace::` doesn't replace it; it
   gives you a place to put it. A `TracedProvider` becomes something you
   call *from inside* a `t.step` closure.

2. **How much replay do you need?** If you've been getting by with
   "run it again from the start", you don't need `trace::`'s replay. If
   you've been building ad-hoc checkpoint files, you probably do.

3. **How many agents do you have?** `trace::` has a fixed cost — you have
   to learn the vocabulary and structure your code around it. That cost
   amortizes over many agents. For a single agent, your own crate is
   probably fine.

The realistic migration path is: **keep your `TracedProvider`, use
`trace::` for the agent shell around it.** Your provider knows about
tokens and rate limits and prompt formatting. `trace::` knows about
steps and delegations and replay. They compose.

## vs. LangGraph

LangGraph is Python, graph-first, and stateful by default. The mental
model is "define nodes and edges, run the graph, inspect the state."

**Where they overlap:**

- Both have first-class state persistence
- Both support checkpoint/resume
- Both model branching and delegation

**Where they diverge:**

| LangGraph | `trace::` |
|-----------|-----------|
| Graph is data, defined before execution | Trace is code, built during execution |
| State is a shared mutable object | Trace is a value that flows through `?` |
| Edges are conditions in Python | Branches are Rust types with compile-time exhaustiveness |
| Node = function in a registry | Step = closure inside a function |
| Checkpoint = full state serialization | Checkpoint = trace snapshot + input hashes |

The philosophical difference: **LangGraph models an agent as a graph
whose nodes you write; `trace::` models an agent as a function whose
control flow you write.**

LangGraph's approach is more declarative and easier to visualize up
front. You draw the graph, you know what's going to happen. `trace::`'s
approach is more dynamic — the actual shape of the trace depends on
which branches fire at runtime. It's harder to reason about a priori,
but you get to use `if`, `match`, and early return like normal code.

**When to use which:**

- LangGraph if you want a visual graph editor or non-engineers authoring
  flows, or if you're already on Python.
- `trace::` if you're in Rust, you want compile-time exhaustiveness on
  your branches, and you like reasoning about agents as functions rather
  than graphs.

## vs. CrewAI / AutoGen

These are higher-level than `trace::`. They model the world as "a crew of
agents with roles, passing messages." The agent identity (the Role, the
Goal, the Backstory) is the primary abstraction.

**Where they overlap:**

- Both let you compose multiple agents into a system
- Both have delegation
- Both have some form of failure handling

**Where they diverge:**

| CrewAI / AutoGen | `trace::` |
|-------------------|-----------|
| Agent = role + prompt template | Agent = `impl Agent` trait |
| Communication = messages | Communication = `delegate` return values |
| No first-class trace | Trace is the return value |
| State in the conversation | State in `Task<S>` + `Trace<T>` |
| Python, dynamic | Rust, typed |

The honest comparison is that `trace::` and CrewAI are solving adjacent
problems. CrewAI is about *how agents talk to each other*. `trace::` is
about *how one agent's execution is recorded and recovered*. You could
build a CrewAI-like framework on top of `trace::` — it would be a crate
that provides opinionated `Agent` impls and a delegation pattern. And
you could implement `trace::`'s recording and replay inside CrewAI — it
would be a plugin that intercepts every tool call.

**When to use which:**

- CrewAI/AutoGen if your problem is "orchestrate many agents with
  different personas and have them collaborate."
- `trace::` if your problem is "run one agent reliably, with recovery
  and replay."
- Both if you want the CrewAI-style orchestration layer on top of
  `trace::`-style reliability.

## vs. Temporal / Durable Execution frameworks

Temporal (and its cousins — Restate, Durable Functions) are the gold
standard for "long-running workflows that survive crashes". If you've
used Temporal, a lot of `trace::` will feel familiar.

**Where they overlap:**

- Both treat execution history as a first-class value
- Both support deterministic replay from history
- Both checkpoint at the boundaries of what they call "activities" or
  "steps"

**Where they diverge:**

| Temporal | `trace::` |
|----------|-----------|
| Separate workflow service | Lives in your process |
| Activities are RPC calls | Steps are closures in your function |
| Determinism is enforced by API restrictions | Determinism is enforced by input hashes |
| Multi-language via SDKs | Rust only |
| Scales to millions of workflows | Scales to hundreds of thousands, locally |

The one-liner: **Temporal is a durable execution *service*; `trace::`
is a durable execution *library*.**

If you have many workflows, many hosts, and strict SLAs, Temporal is
the right answer and `trace::` is not competing with it. If you have a
handful of agents on a single host and you want durable execution
without running an extra service, `trace::` is the more pragmatic
choice.

You can use both: run Temporal as the top-level orchestrator, and use
`trace::` inside a Temporal activity to record the LLM call graph. The
activity's inputs and outputs are what Temporal sees; the `Trace<T>` is
what you look at to debug the LLM behavior.

## Summary table

| Tool | Best at | Weakness | `trace::` relationship |
|------|---------|----------|-----------------------|
| `tokio::spawn` + channels | Raw flexibility | No structure, no replay | `trace::` structures this for agents |
| `tracing` crate | Observability for services | No replay, no confidence | Use alongside; `trace::` can emit `tracing` events |
| Your own `agent_trace` | Domain-specific providers | Reinventing wheel | `trace::` is the shell; keep your providers |
| LangGraph | Visual graph authoring | Python, stateful | Different paradigm; pick one |
| CrewAI / AutoGen | Multi-agent personas | No first-class trace | Adjacent problems; composable |
| Temporal | Durable workflows at scale | Requires a service | `trace::` is the library version |

## The design philosophy

Every one of those tools got something right that `trace::` borrows:

- From `tracing`: structured records with automatic propagation
- From `agent_trace`: traces-as-values, not side effects
- From LangGraph: first-class state and replay
- From CrewAI: agents as a composition primitive
- From Temporal: input-hash-based determinism

And one thing `trace::` deliberately does *not* borrow: **the assumption
that your agent code is special**. There's no special `WorkflowContext`,
no DSL-inside-a-DSL, no "you can't use `if` here". It's just Rust, with a
macro that rewrites your function to record what it does. Everything
else is library.

That's the whole design philosophy. The tutorial is over.
