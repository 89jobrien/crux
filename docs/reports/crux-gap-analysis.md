# Crux DSL Gap Analysis

Source: `maestro_smoketest.crux` vs `crux-script` schema + `crux-stdlib` + `crux-agentic`.

## Current DSL Constructs (supported)

| Construct                                 | Schema type      | Crate        |
| ----------------------------------------- | ---------------- | ------------ |
| `pipeline:` + `steps:`                    | `PipelineDef`    | crux-script  |
| `step:` + `handler:` + `args:`            | `StepNode`       | crux-script  |
| `pipe:` + `stages:`                       | `PipeNode`       | crux-script  |
| `join_all:` + `arms:`                     | `JoinAllNode`    | crux-script  |
| `delegate:`                               | `DelegateNode`   | crux-script  |
| `route_on_confidence:`                    | `RouteNode`      | crux-script  |
| `speculate:`                              | `SpeculateNode`  | crux-script  |
| `budget:` (single constraint)             | `BudgetDef` enum | crux-script  |
| `{{ expr }}` template expansion           | `ExprContext`    | crux-script  |
| `ctrl::noop`, `ctrl::log`, `ctrl::assert` | handlers         | crux-stdlib  |
| `fs::read`, `fs::write`, `fs::glob`       | handlers         | crux-stdlib  |
| `git::diff`, `git::log`, etc.             | handlers         | crux-stdlib  |
| `json::*`, `text::*`                      | handlers         | crux-stdlib  |
| `shell::exec`, `shell::capture`           | handlers         | crux-agentic |

## Gaps (by priority)

### P0 — Parse-level bugs / silent misbehavior

**Compound budget**: Smoketest uses `{ calls: 40, duration_ms: 900000 }`.
`BudgetDef` is `#[serde(untagged)] enum` — only ONE variant matches.
The second constraint is silently dropped. Fix: make `BudgetDef` a struct
with optional fields, or add a `Combined` variant.

**`shell::capture` not in stdlib**: Shell handlers live in `crux-agentic`,
so a simple ops pipeline requires the full agentic stack. Move to stdlib
or a new `crux-shell` crate.

### P1 — Missing step-level controls

**`expect:` clause**: Declarative output assertions on steps.
`expect: { exit_code: 0, stdout_contains: "ok" }`. Currently requires
wiring `ctrl::assert` manually via args expressions.

**`allow_failure:`**: Per-step or per-arm flag. Let `join_all` arms fail
without killing the pipeline. Returns partial results with error metadata.

**Step-level timeout**: `timeout_ms: 30000` per step, independent of
pipeline budget. Currently only pipeline-level budget exists.

### P2 — Ergonomics

**`vars:` / `let:` bindings**: Define variables once, reference across
pipe stages. `{{ vars.SESSION_NAME }}`. Currently each pipe stage must
independently compute shared values.

**`retry:`**: `retry: { count: 3, delay_ms: 5000 }` for flaky checks.
No retry primitive exists today.

### P3 — Error handling

**`on_error:` handler**: Run a cleanup/fallback step when a step fails.
Currently failure is terminal for the enclosing pipe/pipeline.

## Loops

Four loop primitives, ranked by value for ops/agentic pipelines.

### `poll:` (do-while / retry-until) — highest value

The most common pattern. "Run steps repeatedly until a condition is met
or budget exhausted." Directly replaces shell-level polling (e.g.
`wait_for_pod` in the smoketest).

```yaml
- poll: wait_for_pod
  steps:
      - step: check_phase
        handler: shell::capture
        args:
            cmd: "kubectl get pod -l session=foo -o jsonpath='{..phase}'"
  until: "{{ steps.check_phase.output == 'Running' }}"
  interval_ms: 5000
  max_attempts: 60
```

- Executes at least once (do-while semantics)
- `steps:` array — multi-step loop body
- Each iteration is a traced sub-step (observable, replayable per-tick)
- Budget ticks per iteration (calls + duration enforced)
- `until:` evaluated against ExprContext after each iteration
- Replay: iterations are NOT cached (state-dependent by nature)

`poll` is distinct from `retry`. `poll` checks a condition on the
_output_ — "keep going until X is true." `retry` (P2 gap) re-runs on
_failure_ — "try again if the step errored." They compose: a step
inside a `poll` body could have `retry: 3` for transient errors,
while the outer `poll` waits for the desired state.

### `for_each:` (map over collection)

"Run steps once per item in an array." Sequential by default,
`parallel: true` for bounded concurrency.

```yaml
- for_each: cleanup_sessions
  items: "{{ steps.list_stale.output }}"
  as: session
  steps:
      - step: delete_via_api
        handler: shell::capture
        args:
            cmd: "curl -X DELETE .../{{ iter.session.name }}"
      - step: delete_sts_fallback
        handler: shell::capture
        args:
            cmd: "kubectl delete sts {{ iter.session.name }}"
        allow_failure: true
  parallel: true
  break_if: "{{ iter.index >= 100 }}" # safety cap
```

- `steps:` array — multi-step body per iteration
- Iterator variable bound as `{{ iter.<as_name> }}` (or `{{ iter.item }}`
  if `as:` omitted)
- `{{ iter.index }}` always available (0-based)
- Each iteration is a traced sub-step named `<label>[<index>]`
- Replay: YES if items array is identical (deterministic mapping)
- `allow_failure: true` collects partial results
- `break_if:` — evaluated after each iteration, exits loop early
- `parallel: true` — concurrency cap is dynamic:
    - Default: system-determined (runtime picks based on available
      resources, handler type, budget remaining)
    - Explicit override: `max_concurrency: 8`
    - Config-level default: `crux.parallel.max_concurrency` in
      Cruxfile or runtime config

### `while:` (pre-condition loop)

Useful for convergence loops where the condition is checked BEFORE each
iteration (vs `poll` which runs at least once).

```yaml
- while: converge
  condition: "{{ steps.converge.output.remaining > 0 }}"
  steps:
      - step: process_batch
        handler: batch::process
  break_if: "{{ iter.index >= 50 }}" # safety cap
```

- Zero iterations possible (condition false on entry)
- `steps:` array — multi-step loop body
- `break_if:` supported (same as `for_each`)
- Budget-bounded like all loops

### `repeat:` (fixed N iterations)

Simplest form. Run exactly N times. Useful for load testing, warmup,
or retry-with-transform patterns.

```yaml
- repeat: warmup
  count: 3
  steps:
      - step: hit_health
        handler: shell::capture
        args:
            cmd: "curl -s https://api.example.com/health"
      - step: log_result
        handler: ctrl::log
  break_if: "{{ steps.hit_health.output == '' }}" # abort on empty
```

- `steps:` array — multi-step body
- `{{ iter.index }}` available (0-based)
- `break_if:` supported
- Trace records N sub-steps (or fewer if `break_if` fires)

### Design constraints (all loops)

- Every iteration MUST be a traced sub-step (replay, budget,
  observability)
- All loops use `steps:` (array) for the body — no single-step sugar
- Budget integration: runaway loops respect `calls:` / `duration_ms:`
- Expression evaluation: `until:`, `condition:`, `items:`, `break_if:`
  all use ExprContext
- `break_if:` is optional on all loop types
- `{{ iter.index }}` is always available inside any loop body
- `for_each` + `poll` cover ~90% of real-world loop needs
- `while` and `repeat` are lower priority but complete the model

### Concurrency model (`for_each` parallel)

Max concurrency is resolved in priority order:

1. Explicit `max_concurrency:` on the `for_each` node
2. Runtime config (`crux.parallel.max_concurrency`)
3. System-determined default (runtime inspects CPU count, handler
   type — shell handlers get lower default than pure-compute,
   remaining budget headroom)

This keeps simple pipelines zero-config while allowing tuning for
resource-constrained environments or rate-limited APIs.
