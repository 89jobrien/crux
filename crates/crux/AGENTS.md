---
crate: crux
role: facade
owns_logic: false
common_tasks:
  - task: "Add a new combinator"
    location: "crux-runtime/src/ctx.rs"
  - task: "Add a new step type"
    location: "crux-types/src/step.rs"
  - task: "Add a proc macro option"
    location: "crux-macros/src/parse.rs"
  - task: "Add a pipeline handler"
    location: "crux-agentic or crux-stdlib"
  - task: "Add integration tests"
    location: "crates/crux/tests/"
commands:
  ci: "just ci"
  test: "cargo nextest run -p crux"
---

# Agents: crux

## For AI Agents Working With This Crate

This is the **facade crate**. Do not add logic here — it belongs in
`crux-runtime` or `crux-macros`.

### Quick Reference

- To understand runtime behavior: read `crux-runtime`
- To understand macro expansion: read `crux-macros`
- To add a new pipeline handler: add to `crux-agentic` or `crux-stdlib`
- To add a new wire type: add to `crux-types`
