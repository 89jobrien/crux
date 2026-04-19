# 03 — Branching & delegation

> Goal: know when to use `match`, `route_on_confidence`, `speculate`, and
> `delegate`, and why each one is a separate language construct instead of
> just an `if`.

Regular Rust gives you `if`, `match`, and `tokio::spawn`. `cruxai::` gives
you four additional primitives that each correspond to a different _kind_ of
decision an agent makes. The four exist because they have different replay,
budget, and recovery semantics — not because any one of them is "better".

## The four branching kinds

| Primitive               | When to use                             | Records in crux as                           |
| ----------------------- | --------------------------------------- | -------------------------------------------- |
| `match` (plain Rust)    | Pure pattern match on known-shape data  | `Step { kind: Branch, ... }`                 |
| `x.route_on_confidence` | Dispatch on a model's confidence score  | `Step { kind: Branch, ... }` with the score  |
| `x.speculate`           | Run several approaches, pick the winner | Winner as `Ok`, losers as `Rejected`         |
| `x.delegate::<A>`       | Hand off to a separate agent            | `Step { kind: Delegation, children: [...] }` |

Pick the primitive by asking: _what does "failure" mean here?_

- `match` — failure is impossible, or handled by another arm
- `route_on_confidence` — failure means "we weren't sure enough"
- `speculate` — failure means "none of the approaches worked"
- `delegate` — failure means "the sub-agent couldn't handle it"

Each one leaves a different shape of record in the crux.

## Value branching with `match`

No macro needed. Just `match`:

```rust
#[cruxai::agent]
async fn classify(doc: Document) -> Crux<Category> {
    let embedding = x.step("embed", || embed(&doc)).await?;

    let category = x.step("match", || async {
        match embedding.topic {
            Topic::Code     => Ok(Category::Technical),
            Topic::News     => Ok(Category::Current),
            Topic::Story    => Ok(Category::Creative),
            Topic::Unknown  => Err(CruxErr::low_confidence("match", 0.3, 0.8)),
        }
    }).await?;

    Ok(category)
}
```

The `match` is inside `x.step`, so the step records _which_ arm fired (via
the closure's output). The unknown case fails the step with a `LowConfidence`
error — which will trip any `on_low_confidence` hook attached upstream.

**Rule of thumb:** use plain `match` when the arms are pure data decisions.
If any arm involves calling out to another model, reach for
`route_on_confidence` or `delegate` instead.

## Confidence branching with `route_on_confidence`

This is the primitive that makes `cruxai::` feel agentic rather than
procedural. You hand it a score and a set of arms keyed by threshold:

```rust
#[cruxai::agent]
async fn answer(question: String) -> Crux<Answer> {
    let draft = x.step("draft", || quick_draft(&question)).await?;

    x.route_on_confidence(draft.confidence, [
        (0.90.., || async { Ok(draft.into_answer()) }),
        (0.70..0.90, || x.delegate::<Refiner>("refine", &draft).await),
        (0.00..0.70, || x.delegate::<HumanEscalator>("escalate", &draft).await),
    ]).await
}
```

What the compiler checks for you:

1. **Ranges must cover 0.0..=1.0.** A missing range is a compile error. You
   cannot accidentally ship an agent that crashes on `confidence = 0.65`.
2. **Ranges must not overlap.** Overlap is also a compile error — there's no
   ambiguity about which arm runs.
3. **Arms must return the same type.** Exactly like `match`.

What the crux records:

- The score (`0.82`)
- The range that matched (`0.70..0.90`)
- The step name you gave (`"answer_route"` by default)
- The sub-crux of whatever the arm did (because `delegate` is used inside)

When you're debugging later, you can filter `crux.steps.iter().filter(|s|
s.kind == StepKind::Branch)` and see the exact confidence score at each
decision point. That's the kind of thing you'd normally build an entire
eval harness for.

## Speculation with `x.speculate`

Run several approaches concurrently, let them race, pick the best:

```rust
#[cruxai::agent]
async fn finalize(draft: Draft) -> Crux<Itinerary> {
    x.speculate("finalize", [
        ("cheap", || async { finalize_cheap(&draft).await }),
        ("fast",  || async { finalize_fast(&draft).await }),
        ("safe",  || async { finalize_safe(&draft).await }),
    ])
    .with_budget(Budget::tokens(8000))
    .pick_best_by(|r| r.confidence)
    .await
}
```

Three terminators, each with different semantics:

| Terminator                | Picks                                       | Cancels losers? |
| ------------------------- | ------------------------------------------- | --------------- |
| `.first_ok()`             | First arm to succeed                        | Yes, via drop   |
| `.pick_best_by(f)`        | Highest `f(result)` after all finish        | No              |
| `.pick_best_by_racing(f)` | Highest `f` among those that finish in time | Yes             |

### What speculation records

This is the interesting bit. The crux records:

- The winner as a normal `Ok` step
- **Every loser as a `Rejected` step**, complete with its own sub-crux

That means you can replay `crux.rejected_branches()` later, or pipe them
into an eval dataset. Nothing is thrown away silently. If you've ever
built an LLM app where you wished you had a record of "what other options
did the model consider?", this is that.

### Budget sharing across arms

`with_budget` applies to the _whole speculation_, not per arm. Arms share a
single budget pool. If `cheap` burns 6000 tokens, `fast` and `safe` get
2000 between them. This is the one place in `cruxai::` where arms are _not_
independent — speculation is explicitly cooperative.

## Delegation with `x.delegate::<A>`

Delegation is a handoff to another `Agent`. It's the only primitive that
crosses the "who owns this decision" boundary:

```rust
#[cruxai::agent]
async fn plan_trip(goal: String) -> Crux<Itinerary> {
    let research = x.step("research", || search_web(&goal)).await?;

    let draft = x.delegate::<DraftAgent>("draft", research)
        .with_budget(Budget::tokens(4000))
        .on_low_confidence(0.7, |score, ctx| async move {
            ctx.delegate::<HumanReviewer>("human", score).await
        })
        .on_step_failure(|err, ctx| async move {
            ctx.delegate::<SafeDraftAgent>("safe_draft", err).await
        })
        .await?;

    Ok(draft.into_itinerary())
}
```

### What makes `delegate` a language construct

Three things that are painful to get right by hand:

1. **Crux context crosses the boundary.** The child agent runs with its
   own `CruxCtx`, but that context carries the parent's `CruxId` as
   `parent`. When the child finishes, its `Crux<_>` is appended to
   `parent.children`. `crux.causal_chain()` walks across the boundary
   transparently.

2. **Lifecycle hooks attach per call site.** Same `DraftAgent`, two
   different call sites with two different `on_low_confidence` handlers.
   The hooks live on the _builder_, not the agent type — so the agent
   stays reusable.

3. **Budgets compose.** Parent has 10k tokens. Delegation gets 4k.
   Inside the child, speculation gets 3k. The runtime tracks all three
   and fails with a specific `BudgetExceeded` if any is exceeded.

### Delegation vs. calling another `#[cruxai::agent]` function directly

You _can_ just call another agent function:

```rust
let sub_crux = drafter(input).await;
let draft = sub_crux.value()?;
```

That works, and the child crux rolls up into the parent automatically.
But you don't get:

- Budget scoping per call site
- Per-call-site lifecycle hooks
- The `Delegation` step kind (which eval tooling keys off)

Use a direct call for "this is a helper I wrote five minutes ago". Use
`delegate` when the sub-agent is a genuine boundary in the design —
different author, different budget, different failure modes.

## Composition operators

These aren't branching primitives, but they show up often enough in
branching code that they belong in this chapter.

### Pipe operator `|` (sequential)

```rust
let crux = drafter(input) | refiner() | finalizer();
```

Desugars to:

```rust
let t1 = drafter(input).await;
let t2 = refiner(t1.value()?).await;
let t3 = finalizer(t2.value()?).await;
merge_cruxs(&[t1, t2, t3])
```

Errors short-circuit. Cruxs concatenate. Useful for linear pipelines.

### `Crux::join_all` (parallel fan-out)

```rust
let results: Crux<Vec<Answer>> = Crux::join_all(
    questions.into_iter().map(|q| answer(q))
).await;
```

All sub-agents run concurrently. The parent `Crux<Vec<Answer>>` carries
every sub-crux as a child. If any child fails, you can choose:

- `.join_all(...)` — propagate the first error
- `.join_all_best_effort(...)` — collect successes, record failures as
  `Rejected` children, never fail the parent

## Check your understanding

- **You're writing a function that dispatches to one of three tools based
  on a model's self-reported confidence.** Which primitive?
  _`route_on_confidence`._
- **You want to try three draft styles and keep the best.** Which one?
  _`speculate` + `pick_best_by`._
- **You want to call a helper you wrote five minutes ago.** Which one?
  _Plain function call. The crux still rolls up._
- **You want a sub-agent with its own budget and human-escalation hook.**
  Which one? _`delegate`._

Chapter **04** is where the tutorial gets useful: we wire in the task
registry so these cruxs can survive a crash.
