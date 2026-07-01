---
crate: crux-planner
pattern: dual-subsystem
subsystems:
  - name: pipeline-planning
    modules: [deterministic, rule_planner, generator, llm]
    flow:
      - "Goal (natural language or structured)"
      - "DeterministicPlanner (Path B) or LlmPlanner (Path A)"
      - "generator::generate_yaml()"
      - "PipelineDef (crux-script compatible)"
  - name: harness-evolution
    modules: [evolution, metrics]
    flow:
      - "RunMetrics (from completed run)"
      - "EvolutionPlanner"
      - "HarnessDiff"
      - "SafetyPolicy check"
      - "EvolutionOutcome"
---

# Architecture: crux-planner

Two independent planning subsystems in one crate.

## Pipeline Planning

```
Goal (natural language or structured)
  -> DeterministicPlanner (rule-based, Path B)
     OR LlmPlanner (BAML-backed, Path A)
  -> generator::generate_yaml()
  -> PipelineDef (crux-script compatible)
```

## Harness Evolution

```
RunMetrics (from completed pipeline run)
  -> EvolutionPlanner
  -> HarnessDiff (incremental profile change)
  -> SafetyPolicy check
  -> EvolutionOutcome (accepted/rejected/pending)
```
