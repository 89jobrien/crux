---
crate: crux-planner
role: planner
subsystems:
  - name: pipeline-planning
    modules: [deterministic, rule_planner, generator, llm]
  - name: harness-evolution
    modules: [evolution, metrics]
howto:
  - task: "Add a deterministic rule"
    steps:
      - "Add rule pattern in rule_planner.rs"
      - "Map to pipeline steps in deterministic.rs"
      - "Test generated YAML via crux-script::load()"
  - task: "Modify evolution logic"
    steps:
      - "Edit evolution.rs (EvolutionPlanner)"
      - "Edit metrics.rs (RunMetrics) if needed"
      - "Test with SafetyPolicy to verify diff approval"
pitfalls:
  - "Generated YAML must be valid for crux-script::load()"
  - "Evolution diffs must pass SafetyPolicy — never bypass"
  - "LLM planner tests require API keys (baml feature)"
---

# Agents: crux-planner

## For AI Agents Working With This Crate

Two subsystems: pipeline planning and harness evolution.

### Common Pitfalls

- Generated pipeline YAML must be valid for `crux-script::load()`
- Evolution diffs must pass `SafetyPolicy` — never bypass
- LLM planner tests require API keys (`baml` feature)
