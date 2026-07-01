---
crate: crux-planner
type: planner
description: "Goal-to-pipeline planner for crux-script"
version: "0.3.0"
edition: "2024"
dependencies:
  - crux-script
  - crux-runtime
  - crux-agentic (optional)
  - serde
planning_paths:
  - name: "Path A (LLM)"
    impl: LlmPlanner
    feature: baml
  - name: "Path B (deterministic)"
    impl: DeterministicPlanner
    feature: null
modules:
  - name: deterministic
    purpose: "Rule-based pipeline generation"
  - name: rule_planner
    purpose: "Pattern-matching rule engine"
  - name: generator
    purpose: "Pipeline YAML generation"
  - name: evolution
    purpose: "EvolutionPlanner for harness evolution"
  - name: metrics
    purpose: "RunMetrics for evolution decisions"
  - name: llm
    purpose: "LLM-backed planner (behind baml feature)"
features:
  - name: baml
    default: false
    effect: "Enables LlmPlanner via crux-agentic"
---

# crux-planner

Goal-to-pipeline planner for crux-script. Two planning paths:

- **Path A (LLM):** `LlmPlanner` delegates to `crux-agentic::planner`
  (requires `baml` feature)
- **Path B (deterministic):** `DeterministicPlanner` — rule-based,
  zero-latency, zero-cost

Also contains the `EvolutionPlanner` for metrics-driven harness profile
evolution.
