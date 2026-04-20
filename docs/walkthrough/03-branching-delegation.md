# 03 — Branching and Delegation

This chapter covers the four primitives for branching control flow in a Crux agent: plain `match`
inside a step, `route_on_confidence`, `speculate`, and `delegate`. Each serves a distinct purpose;
choosing the right one is as important as using it correctly.

---

## 1. Plain `match` inside a step

The simplest branching is a Rust `match` (or `if`) inside a `x.step` closure. Use this when the
branch decision is purely structural — no confidence scores, no sub-agents, no parallel arms.

```rust
let answer = x.step("classify", |_| async move {
    let label = classify_input(&raw)?;
    match label {
        Label::Simple  => produce_direct_answer(&raw).await,
        Label::Complex => produce_detailed_answer(&raw).await,
    }
}).await?;
```

The entire branch is a single recorded step. Both arms must return the same type. The trace shows
one `Step` entry; the path taken is visible in the output value, not as a separate step.

**Rule of thumb:** use plain `match` when the arms are pure data decisions with no external calls.
If any arm involves a model or a sub-agent, reach for `route_on_confidence` or `delegate` instead.

---

## 2. `route_on_confidence`

Use `route_on_confidence` when a numeric confidence score governs which branch runs. It validates
that the routes cover `[0.0, 1.0]` with no gaps and no overlaps; a misconfigured route table
causes a panic or error at runtime, not a silent wrong-path execution.

### Signature

```rust
pub async fn route_on_confidence<T>(
    &mut self,
    name: &str,
    confidence: f32,
    routes: Vec<ConfidenceRoute<'_, T>>,
) -> Result<T, CruxErr>
```

`ConfidenceRoute<'_, T>` is the tuple `(ConfidenceRange, &str, BoxFut<T>)`.

### Building ranges

Ranges are constructed with named constructors. Do not use Rust range syntax (`0.7..0.9`).

| Constructor                          | Bounds   |
|--------------------------------------|----------|
| `ConfidenceRange::inclusive(lo, hi)` | `[lo, hi]` |
| `ConfidenceRange::exclusive(lo, hi)` | `(lo, hi)` |

The ranges must collectively cover `[0.0, 1.0]`. A common layout uses `inclusive` for the top
bucket (so `1.0` is caught) and `exclusive` for the lower ones.

### Example

```rust
let answer = x.route_on_confidence(
    "answer_route",
    draft.confidence,
    vec![
        (
            ConfidenceRange::inclusive(0.9, 1.0),
            "accept",
            Box::pin(async { Ok(draft.into_answer()) }),
        ),
        (
            ConfidenceRange::exclusive(0.7, 0.9),
            "refine",
            Box::pin(async {
                let refined = refine_draft(&draft).await?;
                Ok(refined.into_answer())
            }),
        ),
        (
            ConfidenceRange::exclusive(0.0, 0.7),
            "escalate",
            Box::pin(async {
                let escalated = escalate_to_human(&draft).await?;
                Ok(escalated)
            }),
        ),
    ],
).await?;
```

Each route produces a labelled `Step` in the trace. Only the matching route's future is awaited;
the others are dropped.

### Validation rules

- Ranges must not overlap.
- There must be no gap between adjacent ranges.
- The union must equal `[0.0, 1.0]`.

Violating any rule is a programming error, not a recoverable runtime condition.

---

## 3. `speculate`

Use `speculate` when you want to run multiple strategies and pick among their results. Arms are
named futures; the builder terminator determines the selection policy.

### Arms

Each arm is a `(&str, Pin<Box<dyn Future<...>>>)` tuple:

```rust
x.speculate("finalize", vec![
    ("cheap", Box::pin(async { finalize_cheap(&draft).await })),
    ("fast",  Box::pin(async { finalize_fast(&draft).await })),
    ("safe",  Box::pin(async { finalize_safe(&draft).await })),
])
```

Arms currently run sequentially. Concurrent execution is planned but not yet implemented. The
API surface is identical either way.

### Terminators

| Terminator              | Behavior                                                              |
|-------------------------|-----------------------------------------------------------------------|
| `.pick_best_by(f).await?` | Runs all arms; selects the one with the highest `f(result)` score. |
| `.first_ok().await?`      | Returns the first arm that succeeds; records failures as Rejected.  |

There is no `pick_best_by_racing` terminator.

The winner is recorded with status `Ok`. Each losing arm is recorded as `Rejected` with its
output preserved in the trace, which is useful for debugging why a strategy lost.

### Example — pick_best_by

```rust
let best = x.speculate("finalize", vec![
    ("cheap", Box::pin(async { finalize_cheap(&draft).await })),
    ("fast",  Box::pin(async { finalize_fast(&draft).await })),
    ("safe",  Box::pin(async { finalize_safe(&draft).await })),
])
.pick_best_by(|r| r.confidence)
.await?;
```

### Example — first_ok

```rust
let result = x.speculate("fetch_data", vec![
    ("primary",   Box::pin(async { fetch_from_primary().await })),
    ("secondary", Box::pin(async { fetch_from_secondary().await })),
    ("fallback",  Box::pin(async { fetch_from_fallback().await })),
])
.first_ok()
.await?;
```

---

## 4. `delegate`

Use `delegate` to hand work to a named `Agent` implementation. The sub-agent runs in its own
`CruxCtx`; its resulting `Crux<T>` is appended as a child of the current step in the parent
trace.

### Builder chain

```rust
x.delegate::<AgentType>("step_name", input)
    .with_budget(Budget::tokens(4000))
    .on_low_confidence(threshold, handler)
    .on_step_failure(handler)
    .run()
    .await?
```

The terminal is `.run().await?`. The builder is not a future; calling `.await` directly on it
does not work.

### `DelegationBuilder` methods

| Method                                   | Effect                                                    |
|------------------------------------------|-----------------------------------------------------------|
| `.with_budget(Budget)`                   | Caps token/step/time consumption for the child.           |
| `.on_low_confidence(threshold, handler)` | Invokes handler when child confidence falls below value.  |
| `.on_step_failure(handler)`              | Invokes handler on any step error inside the child.       |
| `.run()`                                 | Consumes the builder and returns the awaitable future.    |

Per-call-site hooks set here override the agent-level hooks defined in the `Agent` trait impl.

### Example

```rust
let draft = x.delegate::<DraftAgent>("draft", research)
    .with_budget(Budget::tokens(4000))
    .on_low_confidence(0.7, |score| async move {
        Recovery::Propagate
    })
    .on_step_failure(|err| async move {
        Recovery::Propagate
    })
    .run()
    .await?;
```

The child's full trace (every step it recorded) is nested inside the parent's trace under the
`"draft"` step. This gives complete replay fidelity without the parent having to know the child's
internals.

---

## 5. CruxCtx combinators

Two additional combinators address sequential pipelines and parallel fan-out.

### `pipe`

`pipe` chains a sequence of named closures over a single value. Each closure receives the output
of the previous one. Each stage is recorded as a child step.

```rust
// Stage type: (&str, Box<dyn FnOnce(T) -> BoxFut<T>>)
let result = x.pipe("process", initial_value, vec![
    ("normalize", Box::new(|v| Box::pin(normalize(v)))),
    ("enrich",    Box::new(|v| Box::pin(enrich(v)))),
    ("score",     Box::new(|v| Box::pin(score(v)))),
]).await?;
```

`pipe` is a method on `CruxCtx`. There is no `|` pipe operator on `Crux<T>` values.

### `join_all`

`join_all` fans out to multiple named futures of the same type and collects all results into a
`Vec<T>`. All arms must succeed; the first error propagates.

```rust
// Arm type: (&str, BoxFut<T>)
let results: Vec<SectionResult> = x.join_all("gather", vec![
    ("intro",      Box::pin(fetch_intro())),
    ("background", Box::pin(fetch_background())),
    ("analysis",   Box::pin(fetch_analysis())),
]).await?;
```

There is no `join_all_best_effort` variant.

---

## Choosing the right primitive

| Situation                                                     | Use                    |
|---------------------------------------------------------------|------------------------|
| Simple structural branch, same output type                    | `match` inside `step`  |
| Confidence score selects exactly one path                     | `route_on_confidence`  |
| Multiple strategies, pick winner by score or first success    | `speculate`            |
| Delegate to a full `Agent` implementation                     | `delegate`             |
| Sequential transformation pipeline                            | `pipe`                 |
| Parallel independent sub-tasks, collect all results           | `join_all`             |

---

## Summary

- `match` inside a step is invisible to the trace as a branch — only the result is recorded.
- `route_on_confidence` requires a complete, non-overlapping cover of `[0.0, 1.0]`. Construct
  ranges with `ConfidenceRange::inclusive` or `ConfidenceRange::exclusive`, never raw range
  syntax.
- `speculate` records all arms; losers are `Rejected` with their output intact. Terminate with
  `.pick_best_by(f).await?` or `.first_ok().await?`. There is no `pick_best_by_racing`.
- `delegate` runs a sub-agent with its own `CruxCtx`. Per-call hooks override agent-level hooks.
  Always end the chain with `.run().await?`, not `.await?` directly on the builder.
- `pipe` and `join_all` are combinators for sequential and parallel composition without
  introducing a separate agent boundary.

Chapter **04** covers the task registry — how to make these traces survive a crash and resume
from a checkpoint.
