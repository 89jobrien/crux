---
name: planning-with-crux
description: >
  Design, author, validate, or review Crux `.crux` pipelines and Rust agents;
  select supported combinators, expressions, handlers, and execution controls.
---

# Planning with Crux

Use YAML-syntax `.crux` pipelines to compose registered handlers. Use
`#[crux::agent]` for custom Rust types or logic.

```yaml
pipeline: inspect
vars:
  root: "{{ input.root }}"
steps:
  - step: status
    handler: git::status
    args:
      cwd: "{{ vars.root }}"
```

Run with `crux run path.crux [input.json]`; validate without execution with
`crux run path.crux --check`.

## Select control flow

- `step`: one handler.
- `pipe`: sequential stages; each receives the previous output.
- `join_all`: concurrent independent arms; values retain declared order.
- `speculate` + `first_ok`: sequential fallback chain.
- `speculate` + `pick_best`: sequentially run every arm and select numeric
  output `score`; missing scores are `0.0`.
- `route_on_confidence`: dispatch one branch from a scored prior step.
- `delegate`: invoke an explicitly registered pipeline agent.
- `poll`, `for_each`, `while`, `repeat`: repeated nested steps. `for_each`
  remains sequential even with `parallel: true`.

See `references/combinator-patterns.md` for examples.

## Handler and data rules

Static `args` are merged under `args`; each top-level step receives the previous
output. Expressions include `{{ input... }}`, `{{ steps.NAME.output... }}`,
`{{ steps.NAME.confidence }}`, `{{ vars.NAME... }}`, and `{{ iter... }}`.
Use a whole-string expression when the value must remain an array, object,
number, or boolean.

Only handlers registered with `handler` and returning confidence can supply
`steps.NAME.confidence`; `handler_value` does not. See
`references/handler-catalog.md` for all built-ins and exact shapes.

## Execution controls

Normal steps support retries, timeout, tolerated failure, postconditions, and a
fallback handler. Pipeline budgets enforce `steps`, `tokens`, `duration_ms`, and
`usd`; `calls` and `cost_cents` are compatibility spellings for `steps` and USD.
A step is reserved before each actual handler invocation. Duration, tokens, and
USD are recorded after a completed invocation, so those dimensions are soft
caps: the invocation that crosses a limit completes, but the next sequential
invocation does not start. Equality with a limit succeeds.

USD budgets fail closed when a handler does not report cost. Register known-free
handlers explicitly so they report zero rather than unknown cost. `delegate`
nodes are the exception: their nested `budget` is currently ignored and delegated
agent work is not metered against the pipeline budget. Use `timeout_ms` for an
enforced per-step wall-clock timeout.

## Checklist

1. Define the final JSON shape.
2. Separate dependent stages from independent arms.
3. Confirm every handler input and `args` shape.
4. Route only from a handler that emits confidence.
5. Add explicit retry, timeout, or recovery where needed.
6. Validate with `crux run FILE --check` before execution.
7. Use `cargo nextest run` for Rust tests.

See `references/rust-agent-patterns.md` for Rust APIs.
