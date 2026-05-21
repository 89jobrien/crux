# 07 — `crux::` vs existing agentic patterns

> Goal: place `crux::` on the map. If you've built agentic systems before,
> this chapter tells you which patterns `crux::` borrows, which it replaces,
> and which it deliberately leaves alone.

Each comparison is side-by-side: the pattern you already know, and what changes
under `crux::`.

---

## vs. hand-rolled `tokio::spawn` + channels

**The hand-rolled version** — every agentic engineer has written some variation of:

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
- Observability calls scattered through the code
- No record of what the agent considered, only what it did

**Under `crux::`:**

```rust
let crux = ctx.step("plan", |ctx| async move {
    plan(ctx, &goal).await
}).await;

if let Err(e) = crux.value() {
    // retry handled by on_step_failure hook; this site only propagates
}
```

What moved:

| Hand-rolled                   | `crux::`                                 |
| ----------------------------- | ---------------------------------------- |
| Event enum per agent          | `Crux<T>` is the universal trace type    |
| Retry logic at receiver       | `on_step_failure` hook at the agent      |
| Observability via `info!`     | Every `ctx.step` is a recorded event     |
| No record of rejected options | `speculate` records losers as `Rejected` |

The tradeoff is that `crux::` enforces its shape — you must structure work as
steps, delegations, and speculations. If your agent is "one big LLM call and
then save the result", `crux::` is overkill and you should stay with plain
`tokio::spawn`.

---

## vs. the `tracing` / `tracing-opentelemetry` crate

This is the closest cousin. `tracing` gives you `Span`, `Event`, and the macro
infrastructure to record them. It is the de facto standard in Rust observability.

**Where they overlap:**

- Both produce structured, hierarchical records
- Both support automatic propagation via task-local context
- Both can be serialized to JSON

**Where they diverge:**

| `tracing`                       | `crux::`                                 |
| ------------------------------- | ---------------------------------------- |
| Spans are observability         | Steps are program state                  |
| `Span` is a side effect         | `Crux<T>` is the return value            |
| Events are fire-and-forget      | Steps hold their output value            |
| No concept of confidence        | `confidence: f32` is a first-class field |
| No concept of rejected branches | `speculate` records losers               |
| Replay is not a goal            | Replay is a language feature             |

The one-sentence version: **`tracing` tells you what happened; `crux::` tells
you what happened _and_ lets you re-run it from a snapshot.**

You cannot replay a `tracing` span back through your code because the span does
not contain the inputs to the functions that ran under it. `crux::` stores an
input hash for each step, so replay via `CruxCtx::replay_from(&snapshot)` is
correct by construction.

**When to use which:**

- Stick with `tracing` for RPC services, web servers, and CI systems — anywhere
  observability is the goal and replay is not.
- Use `crux::` for agents where the decision process is itself the artifact.
- Use both in the same process. When the `tracing` feature flag is enabled,
  `crux::` emits `tracing` span events, so existing APM dashboards keep working
  with no additional instrumentation.

---

## vs. LangGraph

LangGraph is Python-first and graph-first. The mental model is: define nodes and
edges, run the graph, inspect the state machine.

**Where they overlap:**

- Both have first-class state persistence
- Both support checkpoint and resume
- Both model branching and delegation

**Where they diverge:**

| LangGraph                               | `crux::`                                                 |
| --------------------------------------- | -------------------------------------------------------- |
| Graph is data, defined before execution | Trace is code, built during execution                    |
| State is a shared mutable object        | `Crux<T>` is a value that flows through `?`              |
| Edges are conditions in Python          | Branches are Rust types with compile-time exhaustiveness |
| Node = function in a registry           | Step = closure inside a function                         |
| Checkpoint = full state serialization   | Checkpoint = `Crux` snapshot + input hashes              |

The philosophical difference: **LangGraph models an agent as a graph whose nodes
you write; `crux::` models an agent as a function whose control flow you write.**

LangGraph's approach is more declarative and easier to visualize up front. You
draw the graph and know the topology before execution. `crux::`'s approach is
more dynamic — the actual shape of the trace depends on which branches fire at
runtime. That is harder to reason about statically, but you get `if`, `match`,
and early return without any framework-imposed restrictions.

**When to use which:**

- LangGraph if you want a visual graph editor, non-engineers authoring flows, or
  if your stack is Python.
- `crux::` if you are in Rust, want compile-time exhaustiveness on your
  branches, and prefer reasoning about agents as functions rather than graphs.

---

## vs. CrewAI / AutoGen

These operate at a higher abstraction level than `crux::`. They model the world
as "a crew of agents with roles, passing messages." The agent identity — Role,
Goal, Backstory — is the primary abstraction.

**Where they overlap:**

- Both let you compose multiple agents into a system
- Both have delegation
- Both have some form of failure handling

**Where they diverge:**

| CrewAI / AutoGen               | `crux::`                                 |
| ------------------------------ | ---------------------------------------- |
| Agent = role + prompt template | Agent = `impl Agent` trait               |
| Communication = messages       | Communication = `delegate` return values |
| No first-class execution trace | `Crux<T>` is the return value            |
| State in the conversation      | State in `TaskStatus` + `Crux<T>`        |
| Python, dynamic                | Rust, typed                              |

`crux::` and CrewAI are solving adjacent problems. CrewAI is about _how agents
talk to each other_. `crux::` is about _how one agent's execution is recorded
and recovered_. You could build a CrewAI-style framework on top of `crux::` —
it would be a crate that provides opinionated `Agent` impls and a delegation
pattern. And you could implement `crux::`-style recording inside CrewAI — it
would be a plugin that intercepts every tool call.

**When to use which:**

- CrewAI/AutoGen when your problem is "orchestrate many agents with different
  personas and have them collaborate."
- `crux::` when your problem is "run one agent reliably, with recovery and
  replay."
- Both when you want the CrewAI-style orchestration layer on top of `crux::`
  reliability.

---

## vs. Temporal / Durable Execution frameworks

Temporal (and its cousins — Restate, Azure Durable Functions) are the gold
standard for "long-running workflows that survive crashes." If you have used
Temporal, much of `crux::` will feel familiar.

**Where they overlap:**

- Both treat execution history as a first-class value
- Both support deterministic replay from history
- Both checkpoint at the boundaries of what they call "activities" or "steps"

**Where they diverge:**

| Temporal                                  | `crux::`                                |
| ----------------------------------------- | --------------------------------------- |
| Separate workflow service                 | Library — lives in your process         |
| Activities are RPC calls across a service | Steps are closures inside your function |
| Determinism enforced by API restrictions  | Determinism enforced by input hashes    |
| Multi-language via SDKs                   | Rust only                               |
| Scales across many hosts                  | Scales on a single host                 |
| External persistence service required     | `redb` embedded backend, no service     |

The one-liner: **Temporal is a durable execution _service_; `crux::` is a
durable execution _library_.**

`crux::` persists checkpoints via its `redb` backend — a pure-Rust embedded
key-value store that writes directly to a local file. There is no separate
service to operate. That simplicity is the point and also the ceiling: if you
need workflows distributed across many hosts with strict SLAs, Temporal is the
right answer and `crux::` is not competing with it.

For a handful of agents on a single host that need durable execution without the
operational overhead of a workflow service, `crux::` is the more pragmatic
choice.

You can use both: run Temporal as the top-level orchestrator, and use `crux::`
inside a Temporal activity to record the LLM call graph. The activity's inputs
and outputs are what Temporal sees; the `Crux<T>` is what you inspect to debug
the LLM behavior.

---

## Summary table

| Tool                      | Best at                    | Weakness                 | `crux::` relationship                              |
| ------------------------- | -------------------------- | ------------------------ | -------------------------------------------------- |
| `tokio::spawn` + channels | Raw flexibility            | No structure, no replay  | `crux::` structures this for agents                |
| `tracing` crate           | Observability for services | No replay, no confidence | Use alongside; enable `tracing` feature for events |
| Your own `agent_trace`    | Domain-specific providers  | Reinventing the wheel    | `crux::` is the shell; keep your providers         |
| LangGraph                 | Visual graph authoring     | Python, stateful         | Different paradigm; pick one                       |
| CrewAI / AutoGen          | Multi-agent personas       | No first-class trace     | Adjacent problems; composable                      |
| Temporal                  | Durable workflows at scale | Requires a service       | `crux::` is the single-host library version        |

---

## The design philosophy

Every one of those tools got something right that `crux::` borrows:

- From `tracing`: structured records with automatic propagation
- From Temporal: input-hash-based determinism and checkpoint/replay
- From LangGraph: first-class state persistence
- From CrewAI: agents as a composition primitive
- From hand-rolled infrastructure: the reality that steps are just closures

And one thing `crux::` deliberately does not borrow: **the assumption that your
agent code is special**. There is no `WorkflowContext` you must thread everywhere,
no DSL-inside-a-DSL, no "you cannot use `match` here." It is Rust, with a macro
that transforms your function to record what it does. Everything else is library.

That is the whole design philosophy. The tutorial is over.
