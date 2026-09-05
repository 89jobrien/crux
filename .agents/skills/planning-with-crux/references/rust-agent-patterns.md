# Rust agent patterns

```rust
use crux::prelude::*;

#[crux::agent]
async fn greet(name: String) -> Crux<String> {
    let upper = x
        .step("uppercase", || async move { Ok(name.to_uppercase()) })
        .await?;
    Ok(format!("Hello, {upper}!"))
}
```

The declared return is `Crux<T>`, but the macro rewrites the body as
`Result<T, CruxErr>`, injects `x: &mut CruxCtx`, generates `GreetAgent`, and
keeps `greet(...) -> Crux<String>` as the wrapper.

## Steps

The facade prelude imports `Context`, which supplies step methods.

```rust
let parsed = x.try_step("parse", || async { serde_json::from_str::<Value>(&raw) }).await?;
let body = x.step_keyed("fetch", &url, || async move { Ok(downloaded) }).await?;
let label = x.step_with_confidence("classify", 0.85, || async { Ok("safe") }).await?;
```

Use `step_retryable` when a hook may return `Recovery::Retry`; a single-shot
step cannot rerun its consumed closure.

## Pipe and join

```rust
let output = x.pipe("clean", input, vec![
    ("trim", Box::new(|s: String| Box::pin(async move { Ok(s.trim().to_owned()) }))),
    ("upper", Box::new(|s: String| Box::pin(async move { Ok(s.to_uppercase()) }))),
]).await?;

let values = x.join_all("fetch", vec![
    ("a", Box::pin(async { Ok::<_, CruxErr>(10) })),
    ("b", Box::pin(async { Ok::<_, CruxErr>(20) })),
]).await?;
```

Pipe is sequential. Join arms are concurrent, all live arms finish, and results
retain input order.

## Speculation

```rust
let best = x.speculate("choose", vec![
    ("a", Box::pin(async { Ok::<_, CruxErr>(Candidate { score: 0.7 }) })),
    ("b", Box::pin(async { Ok::<_, CruxErr>(Candidate { score: 0.9 }) })),
]).pick_best_by(|candidate| candidate.score).await?;
```

Speculation is sequential. `first_ok` short-circuits; `pick_best_by` runs all
and marks successful losers rejected. `pick_best()` reads output `score` or
falls back to serialized output length. All-arm failure returns `StepFailed`,
not an `AllSpeculationsFailed` variant.

## Confidence routing

```rust
let action = x.route_on_confidence("decide", score, vec![
    (ConfidenceRange::exclusive(0.0, 0.5), "review",
     Box::pin(async { Ok::<_, CruxErr>("review") })),
    (ConfidenceRange::inclusive(0.5, 1.0), "accept",
     Box::pin(async { Ok::<_, CruxErr>("accept") })),
]).await?;
```

Routes must exactly cover `[0,1]`.

## Delegation

```rust
let child = x.delegate::<WorkerAgent>("work", input)
    .with_budget(Budget::calls(5))
    .run()
    .await?;
```

The child inherits the planner and receives the budget, but usage is not
automatic. A delegation step and child snapshot are recorded.

## Recovery

```rust
x.on_step_failure(|_err| async {
    Recovery::Substitute(serde_json::json!("fallback"))
});
```

Hooks return `Recovery<Value>`, deserialized to the requested output. Variants
are `Retry`, `RetryWith`, `Substitute`, `Escalate`, `Propagate`, `Skip`, and
`Continue`. `Skip` records skipped and returns an error without a replacement.

## Replay and registry

```rust
let snapshot = completed.to_snapshot()?;
let mut ctx = CruxCtx::new("greet");
ctx.set_replay_mode(ReplayMode::Lenient);
ctx.replay_from(&snapshot);
let value = GreetAgent::run(&mut ctx, "Joe".to_owned()).await;
```

`#[crux::agent(registry = "process")]` adds `run_registered`, which submits,
marks running, executes, marks done or failed, checkpoints, and returns
`(Crux<T>, TaskId)`. `replay = "lenient"` sets generated contexts to lenient.

A harness struct must expose `memory_mb`, `cpu_millicores`, `timeout_seconds`,
and `network_access`; the macro generates serde, `Default`, and `to_profile`.
`#[crux::evolve]` behaves like `agent` and only adds an evolution marker. It
does not inject `EvolutionPlanner`.

Run tests with `cargo nextest run`.
