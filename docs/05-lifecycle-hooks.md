# 05 — Lifecycle hooks

> Goal: use `on_low_confidence`, `on_step_failure`, and `on_budget_exceeded`
> to turn "crash at the error site" into "escalate, retry, or degrade
> gracefully" — without scattering the recovery logic across your business
> code.

Lifecycle hooks are the reason `trace::` treats recovery as a language
feature instead of a pattern. In a regular Rust agent, you'd write:

```rust
match call_model(&input).await {
    Ok(out) if out.confidence < 0.7 => escalate_to_human(&out).await?,
    Ok(out) => out,
    Err(e) => {
        tracing::error!(?e, "call_model failed");
        fallback_model(&input).await?
    }
}
```

That works, but the recovery logic is tangled into the happy path. Three
problems:

1. Every call site re-invents the same `match` pattern.
2. Recovery can't cross a delegation boundary — the hook lives here, but the
   failure happened in a sub-agent.
3. You can't inspect the recovery from outside. There's no way to ask
   "how often did this fallback trigger?" without adding more metrics code.

Hooks solve all three.

## The three hooks

| Hook | Fires when | Signature |
|------|------------|-----------|
| `on_low_confidence(threshold, handler)` | A step finishes with `confidence < threshold` | `async fn(score, ctx) -> Recovery<T>` |
| `on_step_failure(handler)` | A step returns `Err(TraceErr)` | `async fn(err, ctx) -> Recovery<T>` |
| `on_budget_exceeded(handler)` | A step or delegation would push us over budget | `async fn(budget, ctx) -> Recovery<T>` |

All three return a `Recovery<T>`:

```rust
pub enum Recovery<T> {
    Retry,                                // Re-run the same step
    RetryWith(Box<dyn FnOnce() -> BoxFuture<'static, Result<T, TraceErr>>>),
    Substitute(T),                        // Use this value as the step's output
    Escalate(BoxFuture<'static, Result<T, TraceErr>>),
    Propagate,                            // Let the error bubble up
    Skip,                                 // Mark step as Skipped, continue
}
```

`Propagate` is the default — if you don't attach a hook, the error goes
straight to the caller.

## Where hooks attach

Three attachment points, ordered from specific to broad:

### 1. Per call site (on a delegation builder)

```rust
let draft = t.delegate::<Drafter>("draft", input)
    .on_low_confidence(0.75, |score, ctx| async move {
        // Inline recovery just for this call site.
        Recovery::Escalate(Box::pin(ctx.delegate::<Reviewer>("review", score).await))
    })
    .await?;
```

Use this when the recovery behavior is specific to *this* call, not the
agent's general behavior.

### 2. Per agent (on the `Agent` impl)

```rust
impl Agent for Drafter {
    fn on_low_confidence(score: f32) -> Recovery<Self::Output> {
        if score < 0.5 {
            Recovery::Escalate(Box::pin(async { human_review().await }))
        } else {
            Recovery::Retry
        }
    }
}
```

Use this when *every* caller of the agent should get the same recovery
behavior. Call-site hooks override the per-agent ones if both are set.

### 3. Scoped with `t.on_low_confidence`

```rust
#[trace::agent]
async fn session(input: Input) -> Trace<Output> {
    t.on_low_confidence(0.8, escalate_handler);
    t.on_step_failure(retry_handler);

    let a = t.step("a", || step_a()).await?;
    let b = t.step("b", || step_b(&a)).await?;
    let c = t.delegate::<SubAgent>("sub", b).await?;  // hook applies here too

    Ok(c)
}
```

The hooks you register on `t` apply to every subsequent step and every
delegation *inside this agent function*. They don't cross delegation
boundaries — a sub-agent has its own hook stack. This is usually what you
want: the hook is scoped to the function that attached it.

## Worked example: escalation ladder

Here's the classic three-tier escalation (cheap model → expensive model →
human) expressed with hooks instead of nested `if`s:

```rust
#[trace::agent]
async fn answer(question: String) -> Trace<Answer> {
    let draft = t.delegate::<CheapModel>("draft", question.clone())
        .on_low_confidence(0.7, |score, ctx| async move {
            Recovery::Escalate(Box::pin(
                ctx.delegate::<ExpensiveModel>("refine", question.clone())
                    .on_low_confidence(0.9, |_, ctx| async move {
                        Recovery::Escalate(Box::pin(
                            ctx.delegate::<HumanReviewer>("human", question).await
                        ))
                    })
                    .await
            ))
        })
        .await?;

    Ok(draft)
}
```

Read that as: *try cheap; if confidence < 0.7, try expensive; if that's still
< 0.9, get a human.*

The trace records:

- Every tier that fired
- Which confidence scores triggered each escalation
- The final answer and which tier produced it

When you look at this trace two weeks later, you know *exactly* why the
answer came from a human instead of the cheap model.

## Worked example: retry with backoff

`Recovery::Retry` and `Recovery::RetryWith` give you retry logic without
writing a retry loop:

```rust
#[trace::agent]
async fn fetch_data(url: String) -> Trace<Vec<Record>> {
    t.on_step_failure(|err, ctx| async move {
        if ctx.attempt() >= 3 {
            return Recovery::Propagate;
        }
        let wait = Duration::from_millis(100 * (1 << ctx.attempt()));
        tokio::time::sleep(wait).await;
        Recovery::Retry
    });

    let raw = t.step("fetch", || http_get(&url)).await?;
    let parsed = t.step("parse", || parse_records(&raw)).await?;
    Ok(parsed)
}
```

Three things to notice:

1. **`ctx.attempt()`** — each step tracks its own attempt count. You get
   per-step retry state without maintaining it yourself.
2. **Backoff is your code.** The hook runs arbitrary async; `tokio::time`
   works fine.
3. **The trace records every attempt.** Failed attempts become `Step {
   status: Err, attempt: 1 }`, the successful one becomes `Step { status:
   Ok, attempt: 3 }`. When you look at the trace later, the full retry
   history is there.

## Worked example: budget-aware degradation

```rust
#[trace::agent]
async fn generate_report(docs: Vec<Doc>) -> Trace<Report> {
    t.on_budget_exceeded(|budget, ctx| async move {
        // Out of tokens? Fall back to a cheaper path.
        Recovery::Escalate(Box::pin(
            ctx.delegate::<TemplateReport>("template", budget.remaining()).await
        ))
    });

    let summaries = Trace::join_all(
        docs.into_iter().map(|d| summarize(d))
    ).await?;

    t.delegate::<PolishedReport>("polish", summaries)
        .with_budget(Budget::tokens(10_000))
        .await
}
```

When the polish step would exceed budget, the hook swaps in a template-based
report instead. The trace records the budget-exceeded event *and* the
substitution. You can tell, per request, whether it got the polished output
or the template fallback.

## Interaction with the task registry

Hooks and the registry (chapter 04) play nicely:

- **A hook that calls `Recovery::Retry`** bumps the task's `attempts`
  counter in the registry.
- **A hook that returns `Recovery::Escalate`** to a human-review agent
  will typically leave the task in an `AwaitingApproval` status. The
  registry has the full trace, so the human reviewer can see exactly what
  the agent tried.
- **`Recovery::Propagate`** causes the registry to mark the task `Failed`
  with the full error chain preserved.

This is where it clicks: hooks aren't just control flow, they're the
mechanism by which a long-running agent transitions between states in the
registry without you writing that glue code.

## When *not* to use hooks

Hooks are powerful, which means they can hide logic in ways that make
debugging harder if you overuse them. Two anti-patterns:

1. **Hooks that mutate global state.** A hook should be about recovery for
   *this* step, not about bumping a global counter or sending a webhook.
   Put those in `t.step` so they appear in the trace as ordinary steps.
2. **Hooks that silently succeed.** `Recovery::Substitute(default_value)`
   makes every failure look like success in your business code. The trace
   still records the substitution, but callers 500 feet downstream have no
   idea the real step failed. Use it sparingly; prefer `Escalate`.

## Check your understanding

- **Where do you attach a hook that applies to all steps in one function?**
  *Call `t.on_low_confidence(...)` / `t.on_step_failure(...)` inside the
  `#[trace::agent]` function.*
- **Which `Recovery` variant replays the same step?** *`Retry`.*
- **Which one replaces the output without retrying?** *`Substitute(T)`.*
- **What happens if you don't attach any hook?** *Errors propagate to the
  caller via `?`. Low-confidence steps still return their value — the
  threshold only matters if there's a hook.*

Chapter **06** puts all five chapters together into the hands-on project:
a decomposer + executor for task planning.
