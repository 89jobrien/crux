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
    coverage: "LLM planner (BAML-routed; prefers a local Ollama)"
commands:
  default: "cargo nextest run -p crux-planner"
  llm: "cargo nextest run -p crux-planner -- llm_planner_generates"
---

# Testing: crux-planner

## Test Strategy

5 inline test modules + 2 dedicated test files covering both planning
subsystems.

## Running

```bash
cargo nextest run -p crux-planner
cargo nextest run -p crux-planner -- llm_planner_generates   # LLM planner (ignored by default)
```
