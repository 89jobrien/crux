<!-- markdownlint-disable MD024 -->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- `HarnessProfile`, `ResourceHints`, `HarnessDiff`, `EvolutionOutcome` types in `crux-runtime`
  for container/process harness lifecycle management
- `SafetyPolicy` trait (port) -- user-defined diff approval logic; returns Approved, Rejected,
  or RequiresApproval
- `ApprovalGate` trait (hook port) -- called when `SafetyPolicy` returns RequiresApproval
- `on_approval_required` lifecycle hook on `Agent` -- fires before a diff is applied
- `AutoApproveGate` and `TerminalApprovalGate` adapters in `crux-agentic`
- `container::run` and `container::wait` pipeline handlers in `crux-agentic`
- `harness::evolve` and `harness::canary` pipeline handlers in `crux-agentic`
- `EvolutionPlanner` and `RunMetrics` in new `crux-planner` crate -- deterministic,
  metrics-driven harness profile evolution
- `#[crux::harness]` proc macro -- annotates a struct as a managed harness
- `#[crux::evolve]` proc macro -- injects `EvolutionPlanner` + `CruxCtx` into an evolution fn
- `crux <target>` shorthand for `crux run --target <target>`, and `crux <file>` for
  `crux run <file>` -- argv is rewritten before parsing, so existing invocations are
  unaffected
- Cruxfile chapter in the pipeline guide (`docs/pipelines/07-cruxfiles.md`), covering
  targets, dependency resolution, fail-fast, per-target budgets, and the shorthand
- Input fixtures for the harness examples (`examples/harness/input_harness_*.json`)
- Array indexing in expression paths -- `{{ steps.fan.output.0 }}` reaches one arm of a
  `join_all`, whose output is a positional array
- `pipe:` stages are recorded under their own labels, so a stage can reference an earlier
  stage and steps after the pipe can reference any of them
- `HandlerMetadata` for `harness::evolve` and `harness::canary`, so their args are validated
- `examples/enrich_health.crux`, `examples/decompose_spec.crux` and
  `examples/plugin_plan.crux`, the pipelines three existing input fixtures were written for
- An expression reference in the combinators chapter (`docs/pipelines/03-combinators.md`)

### Changed

- Root `Cruxfile` is now a real multi-target Cruxfile mirroring `just ci`
  (`fmt -> lint -> typecheck -> build -> test -> deny -> lint-crux -> ci`); it was
  previously in taskit's format, which neither crux-script schema accepts
- The Cruxfile's `cargo check` target is named `typecheck`, since `crux check` is the
  file validator and subcommands win over targets of the same name
- An unresolvable Cruxfile target now prints the available target list
- `--target` passed alongside a plain pipeline warns that it is ignored

### Fixed

- Bare `crux run` in the repo root panicked: the root `Cruxfile` has no `targets:` key,
  so it fell through to the pipeline parser and hit an `expect`. The Cruxfile now parses;
  the underlying panic on a malformed pipeline is unchanged
- `examples/harness/harness_rollback.crux` failed to parse: a `route_on_confidence`
  branch used a nested `steps:` list, which `RouteBranch` does not model
- `examples/harness/harness_safety_gate.crux` and `harness_evolve.crux` referenced
  `harness::safety_validate` and `harness::approval_check`, neither of which is
  implemented; both now build on registered handlers
- Template delimiter in `crux-script` docs and `CONFORMANCE.md` was written `${{ ... }}`;
  the runtime accepts `{{ ... }}`
- `{{ ... }}` templates in `join_all` arm, `pipe` stage and `route_on_confidence` branch
  args were never expanded -- only plain `step:` args were. Nested args reached their
  handler as the literal template string
- The stub injected for an unregistered handler emitted `confidence` inside its JSON but
  registered via `handler_value`, which sets `HandlerOutput::confidence` to `None`. Any
  pipeline routing on a feature-gated handler failed with "produced no confidence score"
  on a default build
- All five `examples/harness/*.crux` pipelines failed at runtime. They now run on a
  default build, with input fixtures added alongside them
- A string of exactly two adjacent templates (`"{{ a }} vs {{ b }}"`) starts with `{{` and
  ends with `}}`, so the whole-string fast path read it as one template with the nonsense
  path `a }} vs {{ b`. Expansion being best-effort, the string silently survived as literal
  text rather than erroring
- `crux run` panicked with a backtrace on a malformed pipeline, an unreadable input file,
  invalid input JSON, or a bad replay trace. All are reported as errors now
- `crux <target> | head` panicked on SIGPIPE; the default disposition is restored at startup
- `--replay` and `--input` were silently dropped on the Cruxfile path; both warn now

### Removed

- `Cruxfile.v1`, a pipeline mislabelled as a Cruxfile whose header referenced a
  `Cruxfile.crux` that never existed; its content is now the root Cruxfile's targets

## [0.1.0] - 2026-04-12

Initial release.

### Added

- `Crux<T>` execution trace type -- inspectable, serializable, replayable
- `CruxCtx` runtime with `step()`, `delegate()`, `speculate()`, `pipe()`, `join_all()`,
  `route_on_confidence()`, `step_stream()`
- `#[crux::agent]` proc macro with `replay` and `registry` attribute wiring
- `Agent` trait with lifecycle hooks (`on_low_confidence`, `on_step_failure`, `on_budget_exceeded`)
- `Recovery<T>` enum: Retry, RetryWith, Substitute, Escalate, Propagate, Skip, Continue
- `Budget` constraints: tokens, calls, duration, cost, combined
- `DelegationBuilder` with per-call-site budget and hooks, child `CruxCtx`
- `SpeculationBuilder` with `pick_best_by` and `first_ok` strategies
- `ReplayCache` with strict and lenient modes, content-hash identity filtering
- `TaskRegistry<B>` with submit/get/update_status/checkpoint/pending lifecycle
- `InMemoryBackend` adapter
- `RedbBackend` adapter (behind `redb` feature, pure-Rust embedded KV)
- Tracing instrumentation (behind `tracing` feature)
- Checkpoint/resume: `CruxCtx::snapshot()`, `checkpoint_to()`, `resume_from()`
- SOLID decomposition: `HookRegistry`, `StepRecorder`, `ReplayCache`, `Context` trait
- 196 tests (203 with redb feature)
