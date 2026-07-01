---
crate: crux-domain
test_strategy: inline
inline_test_modules: 1
dedicated_test_files: 0
test_areas:
  - module: lib
    coverage: "Action/StepIntent construction, Planner impls"
commands:
  default: "cargo nextest run -p crux-domain"
---

# Testing: crux-domain

## Test Strategy

1 inline `#[cfg(test)]` module covering action/intent construction and
planner trait implementations.

## Running

```bash
cargo nextest run -p crux-domain
```

## What's Tested

- `Action` and `StepIntent` construction
- `PassthroughPlanner` approves all
- `DenyAllPlanner` denies all
- `SimulatePlanner` dry-run behavior
- `PlanResult` aggregation
