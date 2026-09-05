# Pipeline capabilities

Source of truth: `crates/crux-script/src/schema.rs`, `runner.rs`, and
`validator.rs`; handlers are registered by `crux-stdlib`, `crux-agentic`, and
optional `crux-baml`.

| Construct | Keys | Current behavior |
| --- | --- | --- |
| Step | `step`, optional `handler`, `args` | Handler defaults to step name |
| Pipe | `pipe`, `stages` | Sequential; output feeds next stage; stage `allow_failure` is parsed but ignored |
| Join | `join_all`, `arms` | Concurrent; output preserves arm order |
| Speculate | `speculate`, `mode`, `arms` | Sequential; `first_ok` short-circuits, `pick_best` runs all |
| Route | `route_on_confidence`, `value`, `routes` | One branch; ranges exactly cover `[0,1]` |
| Delegate | `delegate`, optional `name`, `budget` | Needs `agent_fn`; parsed budget is ignored |
| Poll | `poll`, `steps`, `until`, optional limits | Do-while |
| For each | `for_each`, `items`, `steps` | Sequential; parallel settings are ignored |
| While | `while`, `condition`, `steps` | Pre-condition loop |
| Repeat | `repeat`, `count`, `steps` | Fixed-count loop |

`for_each: label as item_name` binds `{{ iter.item_name }}`; otherwise use
`{{ iter.item }}`. Loops expose zero-based `{{ iter.index }}`. `for_each`,
`while`, and `repeat` support `break_if`; `poll` uses `until`, optional
`max_attempts`, and `interval_ms`.

A normal step also supports `expect`, `allow_failure`, `timeout_ms`,
`retry: { count, delay_ms }`, and `on_error: { handler, args }`. `expect` checks
`exit_code`, `stdout_contains`, and `stderr_contains`. Pipe stage and join arm
objects accept `allow_failure`, but only join arms honor it; pipe stages still abort
on failure.

Pipeline `vars` resolve once in declaration order. Expressions support `input`,
`steps`, `vars`, and `iter` paths in `{{ ... }}`. Whole expressions return typed
JSON; embedded expressions interpolate text.

`handler` may emit confidence; `handler_value` does not. Pipe adopts the last
stage score, join averages scored arms, routes use the chosen handler score or
the routing score, and speculation has no aggregate score. Pipeline `pick_best`
reads numeric output `score`; missing scores become `0.0` and ties favor the
first arm.

## Budget limitation

The schema accepts `tokens`, `calls`, `duration_ms`, and `cost_cents`, and the
runner installs a `Budget`. Execution does not automatically call
`consume_budget`, count handlers, measure duration, read token use, or track
cost. Pipeline budgets are therefore not effective runtime limits. Delegate
budgets are also ignored by the pipeline runner. Use `timeout_ms` for an
enforced per-step wall-clock timeout.

Default registration includes stdlib, analysis, CI, container, harness, review,
rx, SQLite, task, triage, and raw LLM handlers. `docker` selects Bollard instead
of the mock container client. CLI feature `baml` adds `llm::extract`,
`llm::decompose`, and `llm::plan`. See the handler catalog for exact shapes.
