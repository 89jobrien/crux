---
title: Crux<T>
tags: [type, core, trace]
---
# Crux<T>
**Crate:** [[crux-types]] | **File:** `crates/crux-types/src/crux_value.rs`

Execution trace fused with typed result. Every agent run produces a `Crux<T>`.

## Fields
- `id`: [[CruxId]] -- trace identifier
- `agent`: String -- agent name
- `value`: Result<T, [[CruxErr]]> -- final result
- `steps`: Vec<[[Step]]> -- causal chain (this agent only)
- `children`: Vec<Crux<Value>> -- delegation traces (type-erased)
- `started_at`: DateTime<Utc>
- `finished_at`: Option<DateTime<Utc>>

## Key Methods
- `value()` / `into_value()` -- extract inner result
- `causal_chain()` -- flat step list
- `delegations()` -- zips delegation steps with children
- `rejected_branches()` -- filters StepStatus::Rejected
- `duration_ms()`, `succeeded_count()`, `failed_count()`
- `to_trace_json()` -- presentation format (flattens Result)
- `to_mermaid()` -- Mermaid flowchart, color-coded by status
- `to_snapshot()` -- type-erased checkpoint
