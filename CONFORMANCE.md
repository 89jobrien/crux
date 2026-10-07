# Crux v0.3 conformance

This document defines what a runtime, pipeline author, or integrator must
do to be "Crux v0.3 conformant". The wording follows RFC 2119: MUST,
MUST NOT, SHOULD, MAY.

The normative artifacts are:

- `crates/crux-types/src/` — canonical wire-format types (`Crux<T>`,
  `Step`, `Budget`, `CruxId`, `CruxErr`, `RecoveryKind`).
- `crates/crux-runtime/src/` — runtime traits and context (`Agent`,
  `Context`, `CruxCtx`, `RegistryBackend`, `SafetyPolicy`,
  `ApprovalGate`).
- `crates/crux-macros/src/` — proc macros (`#[crux::agent]`,
  `#[crux::harness]`, `#[crux::evolve]`).
- `crates/crux-script/src/` — YAML pipeline parser, validator, and
  step runner.
- `crates/crux/tests/` — conformance and integration tests that define
  expected runtime behaviour.

Pipeline definitions use the `.crux` file extension (YAML syntax).

## 1. Trace requirements

A trace is the `Crux<T>` value returned by every agent invocation.

1. Every `Crux<T>` MUST contain a `pipeline` name, a `status`, and a
   `steps` vector.
2. Each `Step` MUST have a `name`, `kind`, `status`, and `started_at`
   timestamp. Completed steps MUST also have `ended_at`.
3. `StepKind` MUST be one of: `action`, `delegation`, `speculation`,
   `checkpoint`, `pipe`, `join`, `route`, `gate`.
4. `StepStatus` MUST be one of: `running`, `succeeded`, `failed`,
   `errored`, `skipped`, `rejected`.
5. Step names MUST be unique within a single trace. The runtime MUST
   reject duplicate step names with a `CruxErr::StepFailed` error.
6. `CruxId` values MUST use ULID format with a type prefix (`crux_`,
   `task_`). IDs MUST be globally unique across traces.

## 2. Agent requirements

An agent is any type implementing the `Agent` trait or annotated with
`#[crux::agent]`.

1. Agents MUST implement `name() -> &'static str` and
   `run(&self, ctx: &mut CruxCtx, input: I) -> Result<Crux<O>, CruxErr>`.
2. `#[crux::agent]` on `async fn foo(input: T) -> Crux<U>` MUST
   generate:
   1. An inner function with `CruxCtx` injected as `t`.
   2. A public wrapper that creates `CruxCtx` and calls `finalize()`.
   3. A `FooAgent` struct implementing the `Agent` trait.
3. Agent names produced by the macro MUST convert `snake_case` function
   names to `PascalCase` struct names.
4. Agents MUST honour `Budget` limits. When a budget is exceeded, the
   runtime MUST return `CruxErr::BudgetExceeded` without executing
   further steps.

## 3. Context and combinator requirements

`CruxCtx` is the runtime context passed to every agent.

1. `ctx.step(name, closure)` MUST record a `Step` with kind `action`
   and append it to the trace.
2. `ctx.delegate(agent, input)` MUST record a `Step` with kind
   `delegation`, create a child `CruxCtx` with scoped budget, and
   merge the child trace into the parent.
3. `ctx.speculate(arms)` MUST execute all arms concurrently, mark the
   selected arm `succeeded`, and mark all other arms `rejected`.
   Selection strategies MUST include `pick_best_by` and `first_ok`.
4. `ctx.pipe(stages)` MUST chain sequential closures, recording a step
   per stage with kind `pipe`. Each stage receives the prior stage's
   output.
5. `ctx.join_all(arms)` MUST fan out via concurrent execution, recording
   a step per arm with kind `join`.
6. `ctx.route_on_confidence(ranges)` MUST validate that confidence
   ranges are non-overlapping, gap-free, and cover `[0.0, 1.0]`.
   Invalid ranges MUST produce an error, not silent mis-routing.
7. `DelegationBuilder` MUST support per-call-site budget and hooks via
   a fluent API. Child contexts MUST inherit the parent's hook registry
   unless overridden.

## 4. Replay requirements

Replay enables deterministic re-execution from a cached trace.

1. Steps MUST be matched by name plus ordinal hash computed via
   `hash_step_identity`.
2. In strict mode, a mismatch between the replay cache and the current
   execution MUST produce a `CruxErr` and halt.
3. In lenient mode, a mismatch MUST trigger a forward name scan. The
   scan is the designed recovery path for ordinal shifts, not a
   fallback.
4. Replay cache hits MUST return the cached output without
   re-executing the step closure.

## 5. Registry and persistence requirements

`TaskRegistry<B>` manages task lifecycle backed by a `RegistryBackend`.

1. Two backends MUST be available: `InMemoryBackend` (default) and
   `RedbBackend` (behind the `redb` feature flag).
2. `submit()` MUST assign a `CruxId` with `task_` prefix and persist
   the task in `pending` status.
3. `update_status()` MUST use compare-and-swap semantics. A status
   transition from a stale state MUST fail.
4. `checkpoint()` MUST persist intermediate state without changing
   task status.
5. `pending()` MUST return only tasks in `pending` status, ordered by
   submission time.

## 6. Budget requirements

`Budget` enforces resource limits per agent or delegation.

1. A budget MAY constrain `max_tokens`, `max_steps`, and/or
   `max_duration`.
2. Budget checks MUST occur before each step. Exceeding any limit
   MUST produce `CruxErr::BudgetExceeded`.
3. Delegated agents MUST receive a scoped budget that is a subset of
   the parent's remaining budget.
4. Budget consumption MUST be tracked atomically within a `CruxCtx`.

## 7. Hook and recovery requirements

Lifecycle hooks intercept step execution at defined points.

1. `HookRegistry` MUST support `before_step`, `after_step`, and
   `on_error` hooks.
2. Hook return values MUST use `Recovery<T>`: `Continue`, `Skip`,
   `Retry`, `Escalate`, or `Substitute(T)`.
3. `RecoveryKind` is the serializable subset of `Recovery<T>` (closure
   variants stay in core). Wire-format consumers MUST accept all
   `RecoveryKind` variants.
4. Hooks MUST NOT mutate the trace directly. They influence execution
   only through their `Recovery` return value.

## 8. Pipeline (crux-script) requirements

`.crux` files define YAML-driven pipelines executed by the step runner.

1. Pipelines MUST validate against the crux-script schema before
   execution. Invalid pipelines MUST produce diagnostics via `miette`.
2. Each pipeline step MUST reference a handler registered in the
   `HandlerRegistry`. Unknown handlers MUST produce a validation
   warning, not a silent skip.
3. `HandlerOutput` MUST carry an optional `confidence: Option<f32>`.
   Handlers that do not produce confidence MUST return `None`, not a
   default value.
4. Template expressions (`{{ outputs['alias'].field }}`) MUST resolve
   against the pipeline's `StepState`. Missing aliases or fields MUST
   produce an error.
5. Conditional guards (`if_expr`) MUST support `${{ outputs['alias'] }}`
   syntax. The expression MUST resolve to a string; values `"false"`,
   `"0"`, and `""` are falsy, all others truthy.
6. Pipeline validation MUST detect: missing handlers, unreachable steps
   (via `needs` graph), cyclic dependencies, and duplicate step names.

## 9. Harness and evolution requirements

`HarnessProfile` describes a managed container or process harness.

1. `#[crux::harness]` on a struct MUST require an `image: String` field
   and map additional fields to `HarnessProfile`.
2. `HarnessDiff` MUST describe incremental profile changes. Applying a
   diff MUST produce an `EvolutionOutcome`: `Accepted`, `Rejected`, or
   `RequiresApproval`.
3. `SafetyPolicy` MUST evaluate diffs and return `Approved`, `Rejected`,
   or `RequiresApproval`. The policy MUST NOT modify the diff.
4. `ApprovalGate` MUST be invoked only when `SafetyPolicy` returns
   `RequiresApproval`. The gate MUST block until approval or rejection.
5. `#[crux::evolve]` on `async fn f(metrics: RunMetrics) -> Crux<EvolutionOutcome>`
   MUST inject an `EvolutionPlanner` (as `planner`) and a `CruxCtx`
   (as `x`) into the function body.

## 10. Error requirements

`CruxErr` is the canonical error type across all crates.

1. `CruxErr` MUST implement `miette::Diagnostic` for structured error
   reporting with error codes, help text, and source spans.
2. Error variants MUST include at minimum: `StepFailed`,
   `BudgetExceeded`, `DelegationFailed`, `ReplayMismatch`,
   `ValidationError`.
3. `CruxErr` MUST implement `is_transient()` to classify errors as
   retryable. Network errors and rate limits MUST be transient; schema
   violations and budget exhaustion MUST NOT.
4. Errors MUST be serializable via serde for wire transport in
   `crux-types`.

## 11. BAML handler requirements

`crux-baml` provides LLM-backed structured extraction handlers.

1. `register_extract` MUST register an `llm::extract` handler that
   dispatches to BAML functions by name.
2. `register_extract_with(registry, Option<ClientRegistry>)` MUST
   accept an optional `ClientRegistry` for test injection. When
   `Some`, all BAML calls MUST route through the provided registry.
3. Unknown BAML function names MUST produce a `CruxErr::StepFailed`
   listing all known functions.
4. `register_decompose` MUST register an `llm::decompose` handler
   returning structured task breakdowns.

## 11a. TypeSafe judgment handler requirements

`crux-typesafe` provides `judge::score`, a calibrated judgment backed by the
TypeSafe System One endpoint.

1. `judge::score` MUST declare `ConfidenceCapability::Always`. That declaration
   is what makes *this step's* confidence routable: `Always` is the only
   capability the compiler accepts for `{{ steps.<name>.confidence }}` without
   raising `dynamic_boundary`. It is **not** what first made confidence routing
   possible from YAML — `crux-baml` already declares `Always` on five handlers
   (`llm::invoke`, `llm::invoke_with_fallback`, `llm::stream`, `llm::analyze`, and
   `llm::confidence`), so a pipeline routing on `llm::confidence` is valid
   without this crate. What `judge::score` adds is a *calibrated* confidence to
   route on — a rubric position rather than a model's self-report.
2. The reported confidence MUST be the rubric position normalized onto
   `0.0..=1.0` and MUST be finite and within `0.0..=1.0`. Normalizing is what
   makes one threshold mean the same thing regardless of how many levels a
   rubric declares.
   1. The level count MUST come from the request's `criteria` — the rubric the
      caller declared, and therefore the axis the backend's `score` is expressed
      against. The response `legend` is an echo of `criteria` and MUST NOT
      redefine that axis.
   2. The normalized position MUST be multiplied by the retry discount
      `RETRY_DECAY ^ (attempts - 1)`, where `attempts` is the number of backend
      calls the judgment actually required, counting the successful one. A
      first-try success therefore reports `attempts: 1` and an undiscounted
      `retry_multiplier: 1.0`; an `attempts` of zero MUST be treated as one.
3. `RETRY_DECAY` MUST be `0.8`, matching `crux-baml`, so a score discounted here
   and one discounted there are discounted identically. The payload MUST report
   both `attempts` and the `retry_multiplier` actually applied, so a discounted
   confidence is explainable from the step output alone.
4. The handler payload MUST NOT define a `confidence` key. `crux-baml` reserves
   that name for the step confidence so a payload cannot disagree with the routed
   value; TypeSafe's own concentration metric is published as
   `distribution_confidence` instead.
5. TypeSafe's `confidence` field MUST NOT be used as the routing value. It
   measures distribution concentration, not correctness, and substituting it
   silently changes what a threshold means. It is reported descriptively only.
6. A non-finite `score` MUST produce an error. It MUST NOT be coerced to `0.0`
   (which reads as "maximally unsupported" and misroutes) nor propagated as
   `NaN` (which `HandlerOutput::with_confidence` turns into `None`, which
   `confidence_or_default` then reports as a neutral `0.5` with nothing on the
   wire to explain it).
7. A rubric of fewer than two levels MUST fail before any request is made.
8. The judgment transport MUST be injectable: `register` MUST accept an
   `Arc<dyn JudgmentClient>` so a test can supply `CannedJudgmentClient`, keeping
   the suite runnable under §14.
9. HTTP `429` and every `5xx` status MUST be treated as transient and retried
   with the backoff described by `RetryPolicy`. The TypeSafe documentation and
   both official SDKs retry server-side failures, so failing a retryable `5xx` on
   the first attempt turns a momentary backend blip into a pipeline error. `4xx`
   client errors, including `401` and `422`, MUST fail on the first attempt — a
   rejected key or a rejected rubric is not repaired by repeating the request.
10. `crux-runtime` MUST NOT depend on `reqwest` or on `crux-typesafe`. The
    judgment port lives in `crux-typesafe`, with its HTTP adapter behind that
    crate's `http` feature.
11. `crux-agentic::register_all` and `register_all_with_plugins` MUST register
    `judge::score` if and only if a TypeSafe API key is present in the
    environment, read from `TYPESAFE_API_KEY` — the same `<PROVIDER>_API_KEY`
    convention as `OPENAI_API_KEY` and `ANTHROPIC_API_KEY`. An unset or blank
    key leaves the handler unregistered, so a workspace without TypeSafe
    credentials keeps exactly the handler set it had before this crate existed,
    and a pipeline naming `judge::score` without a key gets an
    `unknown_handler` diagnostic rather than a runtime authentication failure.
    When registered, the client MUST target `https://api.typesafe.ai/v1/systemone`.

## 12. Model ID requirements

`crux-model` provides canonical model identification.

1. Model IDs MUST be parsed from provider-specific formats (OpenAI,
   Anthropic, Ollama) into a canonical `ModelId` type.
2. `ModelId` MUST round-trip through serde serialization without loss.
3. Unknown provider prefixes MUST be preserved as-is, not rejected.

## 13. Versioning

- The crate version is the workspace version — `workspace.package.version` in the
  root `Cargo.toml`, currently `0.4.0`. All workspace crates MUST share it, so a
  crate's version is never stated independently here.
- Adding optional fields to `Step`, `Budget`, or `HarnessProfile` is
  backwards compatible and gets a patch bump.
- Adding new `StepKind` or `StepStatus` variants is backwards
  compatible; consumers MUST handle unknown variants gracefully.
- Removing a field, renaming a variant, or changing the trace structure
  is a breaking change and MUST bump the minor version.
- `crux-types` is the wire-format crate. External consumers (e.g.
  minibox) SHOULD depend on `crux-types` alone to avoid pulling the
  full runtime.

## 14. Test matrix

Conformance is verified by 1183 tests across the workspace's 19 members — the 18
crates under `crates/` plus the `xtask` runner. Seventeen of the crates contain
tests; `crux-macros` is a proc-macro crate with none, and is exercised through
the `crux` facade's integration tests.

| Crate          | Tests | Coverage area                                                       |
| -------------- | ----: | ------------------------------------------------------------------- |
| crux-runtime   |   233 | Runtime context, delegation, speculation, replay, hooks, registry, trust, governance, safety, audit |
| crux-script    |   210 | YAML parsing, compilation, validation, runner semantics (loops, expect, retry, timeout), confidence |
| crux-agentic   |   117 | Handlers, adapters, analysis, CI, sqlite, review, triage, plugin discovery, CLI passthroughs |
| crux (facade)  |   112 | Conformance suite, macros, combinators, delegation, speculation, checkpoints, research pipeline |
| crux-cli       |    72 | `crux` binary: check, handlers, schema, trace, regress, doctor, init |
| crux-baml      |    71 | Mock BAML server, extraction, completion, analyze, planner          |
| crux-stdlib    |    62 | Shell, fs, git, json, text, ctrl handlers                            |
| crux-types     |    55 | Wire types, error classification, ids, budgets, emission, serde     |
| crux-model     |    44 | Model ID parsing, vendor registry, provider refs, serde round-trips |
| crux-typesafe  |    42 | Calibration, wire shapes, retry policy, YAML routing                 |
| crux-improve   |    31 | Self-improvement handlers                                           |
| crux-task      |    26 | Task lifecycle, workflow, backend conformance                        |
| crux-plugin    |    23 | Plugin host, protocol, manifest, bridge, discovery                   |
| crux-planner   |    22 | Deterministic planning, evolution, generator, LLM planner snapshots  |
| crux-regression|    21 | Regression store conformance, harness, artifacts                     |
| crux-domain    |    16 | Domain model types, events, pipeline tests                           |
| xtask          |    16 | Workspace task runner: publish, version                             |
| crux-schema    |    10 | `CruxValue` JSON schema                                              |
| crux-macros    |     0 | (proc-macro; tested via crux facade integration tests)              |

Counts are `cargo nextest list` entries per crate — unit tests plus every
integration-test binary. They are a snapshot of this revision, not a constant;
re-derive rather than trusting a stale copy:

```bash
cargo nextest list --workspace | wc -l
```

All tests MUST pass without API keys or network access. Tests that
previously required live LLM calls MUST use `MockBamlServer` with
canned responses via `register_extract_with`.

Run the full conformance suite:

```bash
cargo nextest run --workspace
```
