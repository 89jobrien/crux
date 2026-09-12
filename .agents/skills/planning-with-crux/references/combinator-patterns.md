# Combinator patterns

## Step and pipe

Top-level steps are sequential. Each receives the preceding output plus static
`args` under `args`. If `handler` is omitted, the step name is the handler name.

```yaml
- step: read
  handler: fs::read
  args: { path: "Cargo.toml" }

- pipe: inspect
  stages:
    - step: status
      handler: git::status
    - step: show
      handler: ctrl::log
      args: { pretty: true }
```

Pipe stages run sequentially and are traced as `inspect::status`, etc.

## Join all

Arms execute concurrently with the same input. Output and traces preserve arm
order. Every dispatched future finishes before an arm error is returned.
`allow_failure: true` converts the arm error to a `failed_allowed` value.

```yaml
- join_all: gather
  arms:
    - step: status
      handler: git::status
    - step: log
      handler: git::log
      args: { n: 5 }
```

## Speculation

Both modes execute arms sequentially.

```yaml
- speculate: fallback
  mode: first_ok
  arms:
    - step: preferred
      handler: fs::read
      args: { path: "local.toml" }
    - step: default
      handler: fs::read
      args: { path: "default.toml" }
```

`first_ok` records failed attempted arms as rejected and stops at success.
`pick_best` runs all arms and reads numeric output `score`; missing scores are
`0.0`, and ties favor the first arm. Rust `SpeculationBuilder::pick_best()` is
different: its unscored fallback is serialized output length.

## Confidence routing

```yaml
- route_on_confidence: decide
  value: "{{ steps.score.confidence }}"
  routes:
    - range: "[0.0, 0.5)"
      label: review
      handler: ctrl::log
    - range: "[0.5, 1.0]"
      label: accept
      handler: ctrl::noop
```

Lower bounds are inclusive; `)` excludes and `]` includes the upper bound.
Ranges must be finite, gap-free, non-overlapping, and cover `[0,1]`. The
referenced step must emit confidence.

## Delegate

```yaml
- delegate: registered_agent_name
  name: child_trace_label
```

Embedding code must call `HandlerRegistry::agent_fn` or
`crux_agentic::register_agent`. Pipeline delegate budgets are parsed but ignored.
This path records a plain step rather than a typed child `CruxCtx` trace.

## Step controls

```yaml
- step: flaky
  handler: shell::capture
  timeout_ms: 30000
  retry: { count: 2, delay_ms: 250 }
  expect: { exit_code: 0, stdout_contains: "ok" }
  on_error:
    handler: ctrl::log
    args: { compact: true }
  allow_failure: true
  args: { cmd: "do-work" }
```

Retries are additional attempts traced as `flaky::attempt1`, etc. `on_error`
runs after retries as `flaky::on_error`; `allow_failure` applies if failure
remains.

## Loops

```yaml
- for_each: inspect as path
  items: "{{ input.paths }}"
  steps:
    - step: exists
      handler: fs::exists
      args: { path: "{{ iter.path }}" }
```

`for_each` is sequential; `parallel` and `max_concurrency` are accepted but not
implemented. `poll` is do-while, `while` checks first, and `repeat` uses `count`.
Loop bodies expose `iter.index`. `for_each`, `while`, and `repeat` support
`break_if`; `poll` uses `until`, optional `max_attempts`, and `interval_ms`.

## Rust equivalents

```rust
let out = x.pipe("transform", input, vec![
    ("trim", Box::new(|s: String| Box::pin(async move { Ok(s.trim().to_owned()) }))),
]).await?;

let values = x.join_all("gather", vec![
    ("a", Box::pin(async { Ok::<_, CruxErr>(1) })),
    ("b", Box::pin(async { Ok::<_, CruxErr>(2) })),
]).await?;
```

Use `cargo nextest run`, not `cargo test`.
