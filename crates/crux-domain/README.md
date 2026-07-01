---
crate: crux-domain
type: domain
description: "Pure domain types for the crux agentic DSL"
version: "0.3.0"
edition: "2024"
dependencies:
  - serde
key_types:
  - Action
  - StepIntent
  - PlanResult
  - Planner (trait)
planners:
  - PassthroughPlanner
  - DenyAllPlanner
  - SimulatePlanner
features:
  - name: tokio-pipeline
    default: false
    effect: "Enables async pipeline module"
---

# crux-domain

Pure domain types for the crux agentic DSL. Zero async, zero LLM
dependencies. External consumers (minibox, slash) can depend on this
crate without pulling tokio or BAML.

## Types

| Type | Purpose |
|------|---------|
| `Action` | Executable action with `StepIntent` |
| `StepIntent` | What a step intends to do |
| `PlanResult` | Outcome of a planning operation |
| `Planner` | Trait for plan generation |

## Planner Implementations

- **`PassthroughPlanner`** — accepts all actions unchanged
- **`DenyAllPlanner`** — rejects all actions
- **`SimulatePlanner`** — dry-run mode
