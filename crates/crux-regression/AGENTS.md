---
crate: crux-regression
role: regression-lifecycle
add_here:
  - "Golden trace artifact storage"
  - "Baseline promotion and history"
  - "Regression evaluation orchestration"
do_not_add:
  - reason: "Pure trace comparison or policy types"
    target: crux-improve
  - reason: "Pipeline execution or replay"
    target: crux-runtime or crux-script
  - reason: "CLI rendering"
    target: crux-cli
---

# Agents: crux-regression

Keep dependency direction inward: `crux-regression` may depend on `crux-improve`, but never on
runtime, script, agentic, or CLI crates. External persistence belongs behind `RegressionStore`.
