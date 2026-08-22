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

## Referencing earlier steps

Any `args:` value can interpolate a `{{ ... }}` expression, resolved
against the pipeline input and the steps that have run so far. This
works in a plain `step:`, in a `join_all` arm, in a `pipe` stage, and in
a `route_on_confidence` branch.

| Path | Resolves to |
| --- | --- |
| `{{ input }}` | the whole pipeline input |
| `{{ input.field.sub }}` | a dot-path into the input |
| `{{ steps.<name>.output }}` | that step's full output |
| `{{ steps.<name>.output.field }}` | a dot-path into its output |
| `{{ steps.<name>.confidence }}` | its confidence score |

A path segment that is a number indexes an array. A `join_all` output is
a positional array in arm order, so that is how you reach one arm:

```yaml
- join_all: gather
  arms:
    - step: baseline
      handler: shell::capture
      args:
        cmd: "echo baseline"
    - step: candidate
      handler: shell::capture
      args:
        cmd: "echo candidate"

- step: compare
  handler: ctrl::log
  args:
    summary: "{{ steps.gather.output.0.stdout }} vs {{ steps.gather.output.1.stdout }}"
```

A `pipe` records each stage under its own label as it completes, so a
later stage can name an earlier one, and so can any step after the pipe:

```yaml
- pipe: analyze
  stages:
    - step: parse
      handler: shell::capture
      args:
        cmd: "echo 42"
    - step: report
      handler: ctrl::log
      args:
        parsed: "{{ steps.parse.output.stdout }}"
```

Two things to know. A whole-string template (`"{{ x }}"`) returns the
typed JSON value; a template embedded in surrounding text interpolates
to a string. And expansion is best-effort: a path that does not resolve
leaves the original text in place rather than failing the step, which is
what lets a pipeline carrying `{{ input.thing }}` still run with no
input at all.

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
