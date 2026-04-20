# 07 — `cruxx::` vs existing agentic patterns

> Goal: place `cruxx::` on the map. If you've built agentic systems before,
> this chapter tells you which patterns `cruxx::` borrows, which it replaces,
> and which it deliberately leaves alone.

Each comparison is side-by-side: the pattern you already know, and what changes
under `cruxx::`.

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

**Under `cruxx::`:**

```rust
let crux = ctx.step("plan", |ctx| async move {
    plan(ctx, &goal).await
}).await;

if let Err(e) = crux.value() {
    // retry handled by on_step_failure hook; this site only propagates
}
```

What moved:

| Hand-rolled                   | `cruxx::`                                  |
| ----------------------------- | ------------------------------------------ |
| Event enum per agent          | `Crux<T>` is the universal trace type      |
| Retry logic at receiver       | `on_step_failure` hook at the agent        |
| Observability via `info!`     | Every `ctx.step` is a recorded event       |
| No record of rejected options | `speculate` records losers as `Rejected`   |

The tradeoff is that `cruxx::` enforces its shape — you must structure work as
steps, delegations, and speculations. If your agent is "one big LLM call and
then save the result", `cruxx::` is overkill and you should stay with plain
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

| `tracing`                       | `cruxx::`                                |
| ------------------------------- | ---------------------------------------- |
| Spans are observability         | Steps are program state                  |
| `Span` is a side effect         | `Crux<T>` is the return value            |
| Events are fire-and-forget      | Steps hold their output value            |
| No concept of confidence        | `confidence: f32` is a first-class field |
| No concept of rejected branches | `speculate` records losers               |
| Replay is not a goal            | Replay is a language feature             |

The one-sentence version: **`tracing` tells you what happened; `cruxx::` tells
you what happened _and_ lets you re-run it from a snapshot.**

You cannot replay a `tracing` span back through your code because the span does
not contain the inputs to the functions that ran under it. `cruxx::` stores an
input hash for each step, so replay via `CruxCtx::replay_from(&snapshot)` is
correct by construction.

**When to use which:**

- Stick with `tracing` for RPC services, web servers, and CI systems — anywhere
  observability is the goal and replay is not.
- Use `cruxx::` for agents where the decision process is itself the artifact.
- Use both in the same process. When the `tracing` feature flag is enabled,
  `cruxx::` emits `tracing` span events, so existing APM dashboards keep working
  with no additional instrumentation.

---

## vs. your own agent trace crate

This section is hypothetical but common enough to be worth naming. If you have
spent any time building agentic infrastructure in Rust, you have probably assembled
something like an `agent_trace` crate — a private, project-specific library that
records what your agents do. The patterns converge because the problems are the
same. The differences tend to be:

| Hypothetical `agent_trace`          | `cruxx::`                                 |
| ------------------------------------ | ----------------------------------------- |
| `AgentRun` struct in your code       | `Crux<T>` as a generic type               |
| Manual `run.record_step(...)` calls  | `ctx.step(...)` with automatic recording  |
| Checkpointing bolted on later        | `#[cruxx::agent(checkpoint_every_step)]`  |
| Provider trait for LLM calls         | `Agent` trait for any delegatable work    |
| Status enum per agent type           | Fixed `TaskStatus` enum, shared           |

**Should you replace your own crate with `cruxx::`?** Three questions decide it:

1. **How much of your crate is domain-specific?** A provider wrapper that
   handles token counting, rate limits, and prompt formatting is something you
   want to keep. `cruxx::` does not replace it — it gives you a place to call it
   from inside a `ctx.step` closure. Your provider becomes a collaborator, not a
   competitor.

2. **How much replay do you need?** If "run it again from the start" has been
   sufficient, you may not need `cruxx::`'s `ReplayCache`. If you have been
   building ad-hoc checkpoint files, `cruxx::` formalizes exactly that.

3. **How many agents do you have?** `cruxx::` has a fixed learning cost — you
   need to understand steps, delegations, and speculations and structure code
   around them. That cost amortizes over many agents. For a single agent, your
   own crate is probably fine.

The realistic migration: **keep your provider, use `cruxx::` as the agent shell
around it.** Your provider knows about tokens and API semantics. `cruxx::` knows
about steps, delegations, and replay. They compose.

---

## vs. LangGraph

LangGraph is Python-first and graph-first. The mental model is: define nodes and
edges, run the graph, inspect the state machine.

**Where they overlap:**

- Both have first-class state persistence
- Both support checkpoint and resume
- Both model branching and delegation

**Where they diverge:**

| LangGraph                               | `cruxx::`                                                |
| --------------------------------------- | -------------------------------------------------------- |
| Graph is data, defined before execution | Trace is code, built during execution                    |
| State is a shared mutable object        | `Crux<T>` is a value that flows through `?`              |
| Edges are conditions in Python          | Branches are Rust types with compile-time exhaustiveness |
| Node = function in a registry           | Step = closure inside a function                         |
| Checkpoint = full state serialization   | Checkpoint = `Crux` snapshot + input hashes              |

The philosophical difference: **LangGraph models an agent as a graph whose nodes
you write; `cruxx::` models an agent as a function whose control flow you write.**

LangGraph's approach is more declarative and easier to visualize up front. You
draw the graph and know the topology before execution. `cruxx::`'s approach is
more dynamic — the actual shape of the trace depends on which branches fire at
runtime. That is harder to reason about statically, but you get `if`, `match`,
and early return without any framework-imposed restrictions.

**When to use which:**

- LangGraph if you want a visual graph editor, non-engineers authoring flows, or
  if your stack is Python.
- `cruxx::` if you are in Rust, want compile-time exhaustiveness on your
  branches, and prefer reasoning about agents as functions rather than graphs.

---

## vs. CrewAI / AutoGen

These operate at a higher abstraction level than `cruxx::`. They model the world
as "a crew of agents with roles, passing messages." The agent identity — Role,
Goal, Backstory — is the primary abstraction.

**Where they overlap:**

- Both let you compose multiple agents into a system
- Both have delegation
- Both have some form of failure handling

**Where they diverge:**

| CrewAI / AutoGen               | `cruxx::`                                |
| ------------------------------ | ---------------------------------------- |
| Agent = role + prompt template | Agent = `impl Agent` trait               |
| Communication = messages       | Communication = `delegate` return values |
| No first-class execution trace | `Crux<T>` is the return value            |
| State in the conversation      | State in `TaskStatus` + `Crux<T>`        |
| Python, dynamic                | Rust, typed                              |

`cruxx::` and CrewAI are solving adjacent problems. CrewAI is about _how agents
talk to each other_. `cruxx::` is about _how one agent's execution is recorded
and recovered_. You could build a CrewAI-style framework on top of `cruxx::` —
it would be a crate that provides opinionated `Agent` impls and a delegation
pattern. And you could implement `cruxx::`-style recording inside CrewAI — it
would be a plugin that intercepts every tool call.

**When to use which:**

- CrewAI/AutoGen when your problem is "orchestrate many agents with different
  personas and have them collaborate."
- `cruxx::` when your problem is "run one agent reliably, with recovery and
  replay."
- Both when you want the CrewAI-style orchestration layer on top of `cruxx::`
  reliability.

---

## vs. Temporal / Durable Execution frameworks

Temporal (and its cousins — Restate, Azure Durable Functions) are the gold
standard for "long-running workflows that survive crashes." If you have used
Temporal, much of `cruxx::` will feel familiar.

**Where they overlap:**

- Both treat execution history as a first-class value
- Both support deterministic replay from history
- Both checkpoint at the boundaries of what they call "activities" or "steps"

**Where they diverge:**

| Temporal                                    | `cruxx::`                               |
| ------------------------------------------- | --------------------------------------- |
| Separate workflow service                   | Library — lives in your process         |
| Activities are RPC calls across a service   | Steps are closures inside your function |
| Determinism enforced by API restrictions    | Determinism enforced by input hashes    |
| Multi-language via SDKs                     | Rust only                               |
| Scales across many hosts                    | Scales on a single host                 |
| External persistence service required       | `redb` embedded backend, no service     |

The one-liner: **Temporal is a durable execution _service_; `cruxx::` is a
durable execution _library_.**

`cruxx::` persists checkpoints via its `redb` backend — a pure-Rust embedded
key-value store that writes directly to a local file. There is no separate
service to operate. That simplicity is the point and also the ceiling: if you
need workflows distributed across many hosts with strict SLAs, Temporal is the
right answer and `cruxx::` is not competing with it.

For a handful of agents on a single host that need durable execution without the
operational overhead of a workflow service, `cruxx::` is the more pragmatic
choice.

You can use both: run Temporal as the top-level orchestrator, and use `cruxx::`
inside a Temporal activity to record the LLM call graph. The activity's inputs
and outputs are what Temporal sees; the `Crux<T>` is what you inspect to debug
the LLM behavior.

---

## Summary table

| Tool                        | Best at                    | Weakness                  | `cruxx::` relationship                              |
| --------------------------- | -------------------------- | ------------------------- | --------------------------------------------------- |
| `tokio::spawn` + channels   | Raw flexibility            | No structure, no replay   | `cruxx::` structures this for agents                |
| `tracing` crate             | Observability for services | No replay, no confidence  | Use alongside; enable `tracing` feature for events  |
| Your own `agent_trace`      | Domain-specific providers  | Reinventing the wheel     | `cruxx::` is the shell; keep your providers         |
| LangGraph                   | Visual graph authoring     | Python, stateful          | Different paradigm; pick one                        |
| CrewAI / AutoGen            | Multi-agent personas       | No first-class trace      | Adjacent problems; composable                       |
| Temporal                    | Durable workflows at scale | Requires a service        | `cruxx::` is the single-host library version        |

---

## The design philosophy

Every one of those tools got something right that `cruxx::` borrows:

- From `tracing`: structured records with automatic propagation
- From Temporal: input-hash-based determinism and checkpoint/replay
- From LangGraph: first-class state persistence
- From CrewAI: agents as a composition primitive
- From hand-rolled infrastructure: the reality that steps are just closures

And one thing `cruxx::` deliberately does not borrow: **the assumption that your
agent code is special**. There is no `WorkflowContext` you must thread everywhere,
no DSL-inside-a-DSL, no "you cannot use `match` here." It is Rust, with a macro
that transforms your function to record what it does. Everything else is library.

That is the whole design philosophy. The tutorial is over.
