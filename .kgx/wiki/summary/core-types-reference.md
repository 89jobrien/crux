---
title: Core Types Reference
source_document: crux_types_crate
tags: [types, reference, wire-format]
---

# Core Types (crux-types)

Minimal-dependency crate. All types are serde-serializable, no async.

## Identity
- [[CruxId]] -- ULID trace ID (prefix `crux_`)
- [[TaskId]] -- ULID task ID (prefix `task_`)

## Execution Trace
- [[Crux<T>]] -- trace + result: id, agent, value, steps, children, timestamps
  - `causal_chain()` -- flat step list
  - `delegations()` -- zips delegation steps with children
  - `to_trace_json()` -- presentation format
  - `to_mermaid()` -- flowchart visualization
  - `to_snapshot()` -- type-erased checkpoint

## Steps
- [[Step]] -- name, [[StepKind]], [[StepStatus]], confidence, timing, hashes,
  output, error, events, metadata, findings
- [[StepKind]] -- Plain / Delegation / Branch / Speculation
- [[StepStatus]] -- Ok / Err / Rejected / Skipped
- [[CitedFinding]] -- diagnostic with source citation
- [[StepState]] -- `HashMap<String, Value>` for pipe stages

## Budget
- [[Budget]] -- Tokens / Calls / Duration / CostCents / Combined
- [[BudgetTracker]] -- remaining / consume / is_exceeded
- [[BudgetKind]] -- discriminant enum

## Errors
- [[CruxErr]] -- StepFailed / LowConfidence / BudgetExceeded / Delegation /
  Cancelled / Denied / ReplayMismatch
  - `is_transient()` -- retryable check
  - `failed_step()` -- recursive extraction

## Recovery
- [[RecoveryKind]] -- serializable: Retry / Skip / Propagate / Continue
- [[FinalPhase]] -- severity-ordered: Succeeded < Skipped < Aborted < Failed < Errored
