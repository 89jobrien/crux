---
crate: crux-agentic
role: handler-library
howto:
  - task: "Add a new handler"
    steps:
      - "Create module in src/ (e.g. my_handler.rs)"
      - "Implement handler matching HandlerRegistry signature"
      - "Add pub mod to lib.rs"
      - "Register in handlers.rs inside register_all()"
      - "Add tests"
  - task: "Add a new ApprovalGate"
    location: "src/adapters/"
  - task: "Add a new SafetyPolicy"
    location: "src/adapters/"
pitfalls:
  - "baml_client/ is generated — never edit manually"
  - "LLM tests require API keys via sops-run"
  - "Handler registration order does not matter"
  - "BAML handlers live in crux-baml, not here"
---

# Agents: crux-agentic

## For AI Agents Working With This Crate

Main handler library. If you need to add a new agentic capability to
pipelines, this is where it goes.

### BAML Handlers

BAML-backed handlers (structured LLM output) live in `crux-baml`, not
here. This crate re-exports them when the `baml` feature is enabled.

### Adapters

`adapters/` contains implementations of runtime ports:
- Add new `ApprovalGate` impls here
- Add new `SafetyPolicy` impls here
