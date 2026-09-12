# Crux architecture

## Boundaries

`crux-types` owns serializable wire data. `crux-domain` owns planning, action,
and event vocabulary. `crux-runtime` owns execution and ports. `crux-script`
interprets `.crux` definitions. `crux-stdlib`, `crux-agentic`, and optional
`crux-baml` provide handlers. `crux-cli` builds the `crux` binary. The `crux`
facade re-exports runtime and macros from package `crux-derive`.

`CruxCtx` contains a `StepRecorder`, `HookRegistry`, `ReplayCache`,
`BudgetTracker`, child traces, a planner, optional event sender, and shared
step-output state. `Agent::run` receives `&mut CruxCtx` directly. The separate
`Context` trait mirrors step, hook, budget, and streaming operations for code
that chooses the abstraction; `Agent` is not generic over `Context`.

Persistence uses `RegistryBackend` (`get`, `put`, `list`, `cas`) with
`InMemoryBackend` and optional `RedbBackend`. `TaskRegistry` adds typed lifecycle
operations and bounded CAS retries.

Safety and approval are separate:

- `SafetyPolicy::validate(diff, base) -> Result<(), SafetyViolation>` and
  `requires_approval(diff) -> bool`.
- `ApprovalGate::request_approval(request) -> ApprovalDecision`.
- `GovernancePolicy` is a serializable struct, not a trait. It checks tools and
  content and composes with most-restrictive-wins semantics.

The terminal approval adapter is in `crux-agentic`; no runtime
`AutoApproveGate` type exists.

## Replay

Each step gets an ordinal-derived `input_hash` from its name and ordinal.
`step_keyed` also stores a content hash.

- Strict replay checks ordinal, name, and hash and reports
  `CruxErr::ReplayMismatch` on divergence.
- Lenient replay first tries the ordinal and then scans forward by name. When
  both sides have content hashes they must match. Hash divergence becomes a
  live miss rather than an error.

`replay_from` seeds top-level steps from `Crux<Value>`. `join_all` allocates all
arm ordinals before dispatch and supports partial replay.

## Pipeline execution

`Runner` validates `PipelineDef`, creates `CruxCtx`, resolves `vars`, and
executes `StepDef` variants. A pipeline `delegate` invokes an agent closure
previously added with `HandlerRegistry::agent_fn`; no agents are built in.

Static `args` are merged under `args`. Expressions use `{{ input... }}`,
`{{ steps.NAME.output... }}`, `{{ steps.NAME.confidence }}`,
`{{ vars.NAME... }}`, and loop-local `{{ iter... }}`. A whole expression
preserves its JSON type; embedded expressions produce text. Argument expansion
preserves the original string when resolution fails.

`handler` returns `HandlerOutput { value, confidence }`; `handler_value` wraps a
plain value with no confidence. `confidence_or_default()` returns neutral `0.5`,
but a route that references missing step confidence fails instead of using it.
