# Crux types and traits

## Wire types

`Crux<T>` fields are `id`, `agent`, `value: Result<T, CruxErr>`, `steps`,
`children`, `started_at`, and optional `finished_at`. Methods include `value`,
`into_value`, `causal_chain`, `delegations`, `rejected_branches`, `duration_ms`,
step counts, `to_trace_json`, `to_mermaid`, and `to_snapshot`.
`causal_chain()` currently returns every top-level step in order, not only
failure-causing steps.

`Step` fields are `name`, `kind`, `status`, `confidence`, `started_at`,
`duration_ms`, `input_hash`, optional `content_hash`, `output`, `error`,
`attempt`, `events`, `metadata`, and `findings`. `StepKind` is `Plain`,
`Delegation`, `Branch`, or `Speculation`; `StepStatus` is `Ok`, `Err`,
`Rejected`, or `Skipped`.

`CruxErr` variants are `StepFailed { step, source_msg }`, `LowConfidence`,
`BudgetExceeded { budget_kind, limit, actual }`, `Delegation { to, source }`,
`Cancelled`, `ReplayMismatch { step, expected, actual }`, and `Denied`.

`Budget` uses struct variants and these constructors:

```rust
Budget::tokens(10_000);
Budget::calls(5);
Budget::duration(std::time::Duration::from_secs(60));
Budget::cost_cents(100);
Budget::combined(vec![Budget::calls(5), Budget::tokens(10_000)]);
```

`BudgetTracker::consume(amount)` applies one scalar amount to every combined
leaf. Exceeded means usage is greater than a limit. It does not measure tokens,
time, calls, or cost automatically. Serializable `RecoveryKind` is `Retry`,
`Skip`, `Propagate`, or `Continue`.

## Runtime API

`Agent` requires serializable, deserializable, sendable input/output, `name`, and
`run(&mut CruxCtx, input)`. `budget()` returns `Budget`, defaulting to unlimited
tokens. It also has default low-confidence and failure hooks.

Bring `Context` into scope (the facade prelude does this) for `step`,
`step_keyed`, `step_with_confidence`, `step_retryable`, `try_step`,
`step_stream`, hooks, and budget methods.

`pipe` is sequential. `join_all` runs arms concurrently, waits for every live
arm, and returns values in input order. `speculate` is sequential:
`first_ok` short-circuits and `pick_best_by` runs all arms. Confidence routes
require finite `[0,1]` input and gap-free, non-overlapping full coverage.

Delegation creates a child context, inherits the planner, optionally sets a
budget, records a `Delegation` step, appends a child snapshot, and wraps failures
as `CruxErr::Delegation { to, source }`.

Runtime `Recovery<T>` is `Retry`, `RetryWith`, `Substitute`, `Escalate`,
`Propagate`, `Skip`, or `Continue`. Repeat behavior requires `step_retryable`;
a retry for a consumed single-shot closure fails. `Skip` records skipped but
returns an error because no typed replacement exists.

## Registry and orchestration

`RegistryBackend` exposes async `get`, `put`, `list`, and `cas`.
`TaskRegistry` provides `submit`, `get`, `update_status`, `checkpoint`, `pending`,
and `load_checkpoint`. `InMemoryBackend` is always available; `RedbBackend`
requires `redb`.

`HarnessProfile` has `id`, `ResourceHints`, `network_access`, and
`allowed_syscalls`. `HarnessDiff` has resource deltas, network change, and
syscall additions/removals. `EvolutionOutcome` is `Promoted`, `Discarded`,
`Blocked`, or `Denied`.

`GovernancePolicy` is data with allow/block/review lists, blocked patterns, and
`max_calls_per_request`. `PolicyAction` is `Allow`, `Deny`, or `Review`.
Approval uses `ApprovalRequest`, `RiskLevel`, `ApprovalDecision`, and
`ApprovalGate`.

## Macros

- `#[crux::agent]` requires an async function declared as `Crux<T>`. Its body is
  rewritten as `Result<T, CruxErr>` with injected `x: &mut CruxCtx`. It creates
  `<PascalName>Agent` and the original wrapper.
- `registry = "kind"` adds `run_registered`; `replay = "lenient"` changes mode.
  `checkpoint_every_step` is parsed but currently generates no behavior.
- `#[crux::harness]` creates serde derives, `Default`, and `to_profile` for a
  named struct. Generated code expects `memory_mb`, `cpu_millicores`,
  `timeout_seconds`, and `network_access` fields.
- `#[crux::evolve]` is agent expansion plus `is_evolution_agent() -> true`; it
  does not inject a planner.
