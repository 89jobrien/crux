# Budgets and errors

## Budgets

Every pipeline declares a budget at the top level:

```yaml
budget: { calls: 10 }
```

Available budget fields:

| Field | Unit | Example |
|-------|------|---------|
| `calls` | Number of steps | `{ calls: 20 }` |
| `tokens` | LLM tokens | `{ tokens: 8000 }` |
| `duration_ms` | Wall-clock milliseconds | `{ duration_ms: 30000 }` |
| `cost_cents` | Cost in cents | `{ cost_cents: 500 }` |

Combine them:

```yaml
budget:
  calls: 20
  tokens: 8000
  duration_ms: 30000
```

When any limit is exceeded, the pipeline stops with a
`BudgetExceeded` error. The trace records which budget was hit.

## What happens when a step fails

If a step returns an error (e.g. `shell::capture` with non-zero exit),
the pipeline stops and reports the failure:

```text
Pipeline: my_pipeline
Status:   FAILED
Duration: 123.4ms
Steps:    3

Trace:
   1. [  OK] read_input (5ms)
   2. [ ERR] bad_command (15ms)
   3. [SKIP] never_ran (0ms)
```

Steps after the failure are marked `SKIP`.

## Assertions

Use `ctrl::assert` to fail explicitly with a message:

```yaml
- step: validate
  handler: ctrl::assert
  args:
    condition: true
    message: "input must not be empty"
```

If `condition` is falsy, the step fails and the pipeline stops.

## Error handling in combinators

- **`join_all`** -- if any arm fails, the whole `join_all` fails.
  Other arms that already completed are still recorded in the trace.
- **`pipe`** -- if any stage fails, the pipe stops. Earlier stages
  are recorded.
- **`speculate: first_ok`** -- arms that fail are skipped; the
  pipeline only fails if all arms fail.
- **`speculate: pick_best`** -- all arms run; the pipeline fails
  only if all arms fail.

## Inspecting failures

Use verbose mode to see error details:

```bash
crux run pipeline.crux -v
```

The trace output includes the error message for failed steps.

Next: [LLM pipelines](./05-llm-pipelines.md).
