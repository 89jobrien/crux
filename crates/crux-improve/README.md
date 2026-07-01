---
crate: crux-improve
type: protocol
description: "Improvement protocol types for the crux agent runtime"
version: "0.3.0"
edition: "2024"
dependencies:
  - crux-types
  - chrono
  - serde
re_exports:
  - "Crux<T>"
  - CruxId
  - Step
  - StepKind
  - StepStatus
---

# crux-improve

Improvement protocol for the crux agent runtime. Defines the vocabulary
for comparing, diffing, and evolving agent execution traces.

## Purpose

Provides types for analyzing and improving agent behavior across runs:
strategies, diffs, comparisons, and improvement policies.

## Re-exports

Re-exports core trace types from `crux-types` so downstream consumers
only need a single dependency:

- `Crux<T>`, `CruxId`, `Step`, `StepKind`, `StepStatus`
