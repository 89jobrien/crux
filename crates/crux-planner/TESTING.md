---
crate: crux-planner
test_strategy: mixed
inline_test_modules: 5
dedicated_test_files: 2
test_areas:
  - module: deterministic
    coverage: "Rule-based pipeline generation"
  - module: rule_planner
    coverage: "Pattern matching and rule evaluation"
  - module: generator
    coverage: "YAML output correctness"
  - module: evolution
    coverage: "EvolutionPlanner with RunMetrics"
  - module: metrics
    coverage: "RunMetrics construction and thresholds"
  - module: llm
    coverage: "LLM planner (requires baml feature + API keys)"
commands:
  default: "cargo nextest run -p crux-planner"
  baml: "cargo nextest run -p crux-planner --features baml"
---

# Testing: crux-planner

## Test Strategy

5 inline test modules + 2 dedicated test files covering both planning
subsystems.

## Running

```bash
cargo nextest run -p crux-planner
cargo nextest run -p crux-planner --features baml   # LLM planner tests
```
