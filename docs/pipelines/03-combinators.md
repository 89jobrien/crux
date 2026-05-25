# Combinators

Plain steps run in sequence. Combinators give you parallel fan-out,
sequential piping, and speculative execution.

## join_all -- parallel fan-out

Run multiple steps concurrently. All arms must succeed.

```yaml
- join_all: gather
  arms:
    - step: fetch_diff
      handler: shell::capture
      args:
        cmd: "wc -l < review_diff.patch"
    - step: fetch_findings
      handler: shell::capture
      args:
        cmd: "cat review_findings.json"
```

The output is a map keyed by step name:

```json
{
  "fetch_diff": { "exit_code": 0, "stdout": "42\n", "stderr": "" },
  "fetch_findings": { "exit_code": 0, "stdout": "[...]", "stderr": "" }
}
```

## pipe -- sequential stages

Chain steps where each stage receives the previous stage's output.

```yaml
- pipe: evaluate
  stages:
    - step: parse
      handler: shell::capture
      args:
        cmd: "cat data.json | jq '.items'"
    - step: count
      handler: shell::capture
      args:
        cmd: "echo 'counted'"
    - step: log_result
      handler: ctrl::log
      args:
        compact: true
```

Use `pipe:` when later stages depend on earlier ones. Each stage sees
the previous stage's output as the pipeline state.

## speculate -- race alternatives

Run multiple approaches and pick a winner.

```yaml
- speculate: strategy
  mode: first_ok
  arms:
    - step: fast_path
      handler: shell::capture
      args:
        cmd: "echo fast"
    - step: thorough_path
      handler: shell::capture
      args:
        cmd: "echo thorough"
```

Two modes:

| Mode | Behavior |
|------|----------|
| `first_ok` | Return the first arm that succeeds |
| `pick_best` | Run all arms, pick highest `score` field |

Losing arms are marked `Rejected` in the trace.

## route_on_confidence -- confidence branching

Route to different handlers based on a confidence score from the
previous step.

```yaml
- route_on_confidence: triage
  routes:
    - range: [0.9, 1.0]
      step: auto_approve
      handler: shell::capture
      args:
        cmd: "echo approved"
    - range: [0.5, 0.9]
      step: human_review
      handler: ctrl::log
    - range: [0.0, 0.5]
      step: reject
      handler: ctrl::assert
      args:
        condition: false
        message: "confidence too low"
```

Ranges must cover `[0.0, 1.0]` with no gaps or overlaps. The previous
step must emit confidence via `HandlerOutput::with_confidence`.

## Nesting

Combinators nest inside each other:

```yaml
- join_all: gather
  arms:
    - pipe: branch_a
      stages:
        - step: a1
          handler: shell::capture
          args:
            cmd: "echo a1"
        - step: a2
          handler: ctrl::log
    - step: branch_b
      handler: shell::capture
      args:
        cmd: "echo b"
```

Next: [Budgets and errors](./04-budgets-and-errors.md).
