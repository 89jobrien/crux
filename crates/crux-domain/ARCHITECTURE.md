---
crate: crux-domain
pattern: pure-domain
constraints:
  - "Zero async"
  - "Zero LLM deps"
  - "All types Send + Sync"
  - "Trait-based extension"
key_traits:
  - name: Planner
    method: "plan(actions) -> PlanResult"
    impls:
      - PassthroughPlanner
      - DenyAllPlanner
      - SimulatePlanner
key_types:
  - name: Action
    contains: ["StepIntent", "metadata"]
  - name: PlanResult
    contains: ["approved", "denied"]
---

# Architecture: crux-domain

Pure domain layer with no infrastructure dependencies.

## Type Graph

```
Planner (trait)
 +-- plan(actions) -> PlanResult
 |
 +-- PassthroughPlanner
 +-- DenyAllPlanner
 +-- SimulatePlanner

Action
 +-- step_intent: StepIntent
 +-- metadata

PlanResult
 +-- approved: Vec<Action>
 +-- denied: Vec<Action>
```

## Design Constraints

- Zero async — all types are `Send + Sync` without requiring tokio
- Zero LLM deps — no API clients, no BAML
- Trait-based extension — add new planners by implementing `Planner`
- Optional `tokio-pipeline` feature adds async pipeline types
