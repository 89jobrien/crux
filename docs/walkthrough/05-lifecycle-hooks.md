# 05 — Lifecycle hooks

> Goal: use `on_low_confidence`, `on_step_failure`, and `on_budget_exceeded` to turn "crash at
> the error site" into "escalate, retry, or degrade gracefully" — without scattering recovery
> logic across business code.

Lifecycle hooks are the reason crux treats recovery as a language feature instead of a pattern.
In a regular Rust agent you write:

```rust
match call_model(&input).await {
    Ok(out) if out.confidence < 0.7 => escalate_to_human(&out).await?,
    Ok(out) => out,
    Err(e) => fallback_model(&input).await?,
}
```

That works, but the recovery logic is tangled into the happy path. Three problems:

1. Every call site re-invents the same `match` pattern.
2. Recovery can't cross a delegation boundary — the hook lives here, but the failure happened
   in a sub-agent.
3. There is no way to ask "how often did this fallback trigger?" without adding metrics code.

Hooks solve all three.

---

## The three hooks

| Hook                                    | Fires when                                  | Default behaviour              |
| --------------------------------------- | ------------------------------------------- | ------------------------------ |
| `on_low_confidence(threshold, handler)` | Step confidence < threshold                 | `Recovery::Continue` (kept)    |
| `on_step_failure(handler)`              | Step returns `Err(CruxErr)`                 | `Recovery::Propagate` (bubble) |
| `on_budget_exceeded(handler)`           | A step or delegation would push over budget | no-op (error propagates)       |

All three handlers return `Recovery<serde_json::Value>`. The type parameter is erased at the
handler boundary because the same hook machinery handles steps of varying output types. The
runtime re-serializes the value back into the expected type after the hook resolves.

### Handler signatures

```rust
// on_low_confidence — receives the actual confidence score
|score: f32| async move { Recovery::<serde_json::Value>::Continue }

// on_step_failure — receives the error that caused the failure
|err: CruxErr| async move { Recovery::<serde_json::Value>::Propagate }

// on_budget_exceeded — receives the Budget that was exceeded
|budget: Budget| async move { Recovery::<serde_json::Value>::Propagate }
```

### The Recovery enum

```rust
pub enum Recovery<T> {
    /// Re-run the same step (up to max_retries).
    Retry,
    /// Re-run with a different closure.
    RetryWith(Box<dyn FnOnce() -> BoxFut<T> + Send>),
    /// Use this value as the step output without retrying.
    Substitute(T),
    /// Run this future as an escalation path.
    Escalate(BoxFut<T>),
    /// Let the error bubble up to the caller.
    Propagate,
    /// Mark the step as Skipped and continue.
    Skip,
    /// Ignore low confidence and keep the value (on_low_confidence only).
    Continue,
}
```

`Continue` is only meaningful for `on_low_confidence` — it says "the score is below threshold,
but proceed anyway." `Propagate` is only meaningful for failure and budget hooks — it says "I
have no recovery, let the error surface."

---

## Where hooks attach

Three attachment points, ordered from most specific to broadest:

### 1. Per call site — on DelegationBuilder

```rust
let draft = x
    .delegate::<Drafter>("draft", input)
    .on_low_confidence(0.75, |score| async move {
        // Fires only for this delegation, not every step in the agent.
        Recovery::Propagate
    })
    .on_step_failure(|err| async move {
        Recovery::Propagate
    })
    .await?;
```

Use this when the recovery is specific to one call, not the agent's general behavior.
`DelegationBuilder` does not expose `on_budget_exceeded` — budget hooks are scoped to the
context (`x`).

### 2. Per agent — on the Agent trait impl

```rust
impl Agent for Drafter {
    type Input = String;
    type Output = Draft;

    fn name() -> &'static str { "drafter" }

    async fn run(ctx: &mut CruxCtx, input: String) -> Result<Draft, CruxErr> {
        // ...
    }

    fn on_low_confidence(_score: f32) -> Recovery<Self::Output> {
        Recovery::Retry
    }

    fn on_step_failure(_err: &CruxErr) -> Recovery<Self::Output> {
        Recovery::Propagate
    }
}
```

Use this when every caller of the agent should get the same recovery behavior. The Agent trait
defaults are `Recovery::Continue` for low confidence and `Recovery::Propagate` for failure.

Note that the per-agent hooks receive `Self::Output` (not erased), so `Substitute(value)` can
return a typed value directly.

Call-site hooks override per-agent hooks when both are set.

### 3. Scoped — on CruxCtx / Context

```rust
#[crux::agent]
async fn session(input: Input) -> Crux<Output> {
    x.on_low_confidence(0.8, |score| async move {
        Recovery::Propagate
    });
    x.on_step_failure(|err| async move {
        Recovery::Retry
    });
    x.on_budget_exceeded(|budget| async move {
        Recovery::Propagate
    });
    x.set_max_retries(3);  // default is 3

    let a = x.step("a", || step_a()).await?;
    let b = x.step("b", || step_b(&a)).await?;
    Ok(b)
}
```

Scoped hooks apply to every subsequent step inside this agent function. They do not cross
delegation boundaries — a sub-agent has its own hook stack. `set_max_retries(n)` sets the
ceiling for `Recovery::Retry` on any step in scope.

---

## Worked example 1: escalation ladder

Classic three-tier escalation (cheap model -> expensive model -> human):

```rust
#[crux::agent]
async fn answer(question: String) -> Crux<Answer> {
    let answer = x
        .delegate::<CheapModel>("draft", question.clone())
        .on_low_confidence(0.7, |_score| {
            let q = question.clone();
            async move {
                // Cheap model was not confident enough. Try the expensive model.
                // The escalation future must return Recovery<serde_json::Value>.
                // Use serde_json::to_value to wrap the result.
                Recovery::Escalate(Box::pin(async move {
                    // In practice, this delegates into a separate CruxCtx.
                    // Here we show the intent; see the full example in
                    // examples/escalation_ladder.crux.
                    let refined = expensive_model_call(&q).await?;
                    serde_json::to_value(refined).map_err(CruxErr::serialization)
                }))
            }
        })
        .await?;

    Ok(answer)
}
```

The crux records every tier that fired, which confidence scores triggered each escalation, and
which tier produced the final answer. Two weeks later you can replay the trace and know exactly
why a human answered instead of the cheap model.

A realistic three-tier version uses nested delegations:

```rust
#[crux::agent]
async fn answer(question: String) -> Crux<Answer> {
    // Tier 1: cheap model
    let result = x
        .delegate::<CheapModel>("t1", question.clone())
        .on_low_confidence(0.7, |_score| {
            let q = question.clone();
            async move {
                Recovery::Escalate(Box::pin(async move {
                    // Tier 2: expensive model (separate ctx created inside delegate)
                    // on_low_confidence on tier 2 escalates to human
                    let v: serde_json::Value = expensive_answer(&q).await?;
                    Ok(v)
                }))
            }
        })
        .await?;

    Ok(result)
}

// Tier 2 agent carries its own escalation to human review
impl Agent for ExpensiveModel {
    // ...
    fn on_low_confidence(_score: f32) -> Recovery<Self::Output> {
        // Escalate to human; Recovery<Self::Output> is typed here
        Recovery::Escalate(Box::pin(human_review()))
    }
}
```

The key insight: each tier's escalation path is its own agent with its own hook. The outer
agent does not need to know how many tiers exist.

---

## Worked example 2: retry with backoff

`Recovery::Retry` and `Recovery::RetryWith` give you retry logic without a manual retry loop:

```rust
#[crux::agent]
async fn fetch_data(url: String) -> Crux<Vec<Record>> {
    // Scoped failure handler: exponential backoff, max 3 attempts.
    x.set_max_retries(3);
    x.on_step_failure(|err| async move {
        // err carries the attempt number via its context
        Recovery::Retry
    });

    // For backoff, RetryWith gives you control over timing:
    x.on_step_failure(|_err| async move {
        Recovery::RetryWith(Box::new(|| {
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                http_get_records().await
            })
        }))
    });

    let raw = x.step("fetch", || http_get(&url)).await?;
    let parsed = x.step("parse", || parse_records(&raw)).await?;
    Ok(parsed)
}
```

`Recovery::Retry` re-runs the same closure. `Recovery::RetryWith` runs a new closure you
supply — use it when the retry should use different parameters (e.g., a fallback URL or a
shorter timeout).

Each step tracks its own attempt count independently. Failed attempts are recorded as
`Step { status: Err, attempt: N }` and the eventual success as `Step { status: Ok, attempt: M }`.
The full retry history is in the trace without any extra instrumentation.

---

## Worked example 3: budget-aware degradation

```rust
#[crux::agent]
async fn generate_report(docs: Vec<Doc>) -> Crux<Report> {
    // When the polishing step would exceed budget, fall back to a template.
    x.on_budget_exceeded(|_budget| async move {
        // Return a pre-built template value serialized to JSON.
        let template = serde_json::json!({ "body": "Summary unavailable.", "template": true });
        Recovery::Substitute(template)
    });

    let summaries = x
        .join_all(docs.into_iter().map(|d| summarize(d)))
        .await?;

    // This step will trigger on_budget_exceeded if it would push over the limit.
    x.delegate::<PolishedReport>("polish", summaries)
        .with_budget(Budget::tokens(10_000))
        .await
}
```

`Recovery::Substitute` replaces the step output with the given value and continues execution.
The crux records both the budget-exceeded event and the substitution, so you can tell per
request whether the polished output or the template fallback was returned.

`Recovery::Skip` is an alternative: it marks the step as `Skipped` and yields `None` (or
the zero value) to the next step. Use `Skip` when the step result is optional; use `Substitute`
when the result is required but you have a known-good fallback.

---

## Interaction with the task registry

Hooks and the task registry (chapter 04) compose naturally:

- `Recovery::Retry` increments the task's attempt counter in the registry.
- `Recovery::Escalate` to a human-review agent typically leaves the task in `AwaitingApproval`.
  The registry holds the full crux so the reviewer can see exactly what the agent tried.
- `Recovery::Propagate` causes the registry to mark the task `Failed` with the full error chain.

Hooks are not just control flow — they are how a long-running agent transitions between registry
states without you writing that glue code.

---

## When not to use hooks

Hooks are powerful, which means overusing them hides logic in ways that make debugging harder.
Two anti-patterns:

**1. Hooks that mutate global state.**
A hook should be about recovery for this step. Global counter bumps, webhooks, and audit log
writes belong in `x.step(...)` calls so they appear in the trace as ordinary steps. Hiding
side effects in a hook makes the trace incomplete.

**2. Hooks that silently succeed.**
`Recovery::Substitute(default_value)` makes every failure look like success to downstream code.
The crux records the substitution, but callers further down the chain have no idea the real
step failed. Use it sparingly; prefer `Escalate` so the trace shows a deliberate recovery path
rather than a silent override.

---

## Summary

| Situation                                    | Reach for                                                            |
| -------------------------------------------- | -------------------------------------------------------------------- |
| One call site needs different recovery       | `.on_low_confidence` / `.on_step_failure` on the builder             |
| All callers should get the same recovery     | Override `on_low_confidence` / `on_step_failure` on `Agent`          |
| All steps in a function share recovery logic | `x.on_low_confidence` / `x.on_step_failure` / `x.on_budget_exceeded` |
| Re-run the same step                         | `Recovery::Retry`                                                    |
| Re-run with different parameters             | `Recovery::RetryWith(...)`                                           |
| Use a known-good fallback value              | `Recovery::Substitute(value)`                                        |
| Delegate to a richer agent                   | `Recovery::Escalate(future)`                                         |
| Step is optional; continue without it        | `Recovery::Skip`                                                     |
| No recovery available                        | `Recovery::Propagate`                                                |
| Below threshold but keep the value           | `Recovery::Continue` (on_low_confidence only)                        |

Chapter **06** puts all five chapters together into the hands-on project: a decomposer and
executor for task planning.
