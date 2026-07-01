---
crate: crux-types
type: types
description: "Serializable wire-format types for the crux agentic DSL"
version: "0.3.0"
edition: "2024"
dependencies:
  - serde
  - chrono
  - ulid
  - thiserror
key_types:
  - "Crux<T>"
  - Step
  - CruxId
  - TaskId
  - Budget
  - CruxErr
  - "Recovery<T>"
  - RecoveryKind
  - HarnessProfile
  - HarnessDiff
  - ResourceHints
  - EvolutionOutcome
features:
  - name: test-utils
    default: false
    effect: "Exposes testing module with builders"
---

# crux-types

Serializable wire-format types for the crux agentic DSL. Minimal
dependencies (serde, chrono, ulid) — no runtime, no async, no LLM deps.

Designed for cross-workspace consumption. External consumers like minibox
depend on this crate to work with crux traces without pulling the full
runtime.

## Key Types

- **`Crux<T>`** — execution trace fused with result
- **`Step`** — recorded unit of work (kind, status, confidence, output,
  children)
- **`CruxId`** / **`TaskId`** — ULID-based identifiers
- **`Budget`** — token/step/time limits
- **`CruxErr`** — error type
- **`Recovery<T>`** — hook return variants (serializable subset:
  `RecoveryKind`)
- **`HarnessProfile`** / **`HarnessDiff`** / **`ResourceHints`** —
  container/process harness types
- **`EvolutionOutcome`** — result of applying a harness diff
