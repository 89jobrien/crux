---
title: Step
tags: [type, core, trace]
---
# Step
**Crate:** [[crux-types]] | **File:** `crates/crux-types/src/step.rs`

Single recorded execution unit in a [[Crux<T>]] trace.

## Fields
- `name`: String
- `kind`: [[StepKind]] (Plain/Delegation/Branch/Speculation)
- `status`: [[StepStatus]] (Ok/Err/Rejected/Skipped)
- `confidence`: f32
- `started_at`: DateTime<Utc>, `duration_ms`: u64
- `input_hash`: u64, `content_hash`: Option<u64>
- `output`: Option<Value>, `error`: Option<String>
- `attempt`: u32
- `events`: Vec<Value> (streaming events)
- `metadata`: HashMap<String, Value>
- `findings`: Vec<[[CitedFinding]]>

## Methods
- `is_ok()`, `is_err()`
