---
crate: crux-runtime
role: core-runtime
key_entry_points:
  - path: "src/ctx.rs"
    purpose: "CruxCtx and all combinators"
  - path: "src/agent.rs"
    purpose: "Agent trait definition"
  - path: "src/registry/mod.rs"
    purpose: "TaskRegistry and RegistryBackend trait"
  - path: "src/prelude.rs"
    purpose: "Re-exports for ergonomic imports"
howto:
  - task: "Add a new port"
    steps:
      - "Define trait in new module"
      - "Add module to lib.rs"
      - "Re-export in prelude"
      - "Provide adapter in crux-agentic"
  - task: "Add a new combinator"
    steps:
      - "Add method to CruxCtx in ctx.rs"
      - "Add unit tests in #[cfg(test)] module"
      - "Add integration test in crates/crux/tests/combinators.rs"
pitfalls:
  - "CruxCtx takes &mut self — combinators must be careful with borrows"
  - "Replay matching uses hash_step_identity — changing step names breaks replay"
  - "Types re-exported from crux-types should not be duplicated here"
---

# Agents: crux-runtime

## For AI Agents Working With This Crate

This is the **core runtime**. Most domain logic lives here.

### Common Pitfalls

- `CruxCtx` takes `&mut self` — combinators must be careful with borrows
- Replay matching uses `hash_step_identity` — changing step names breaks
  replay compatibility
- Types re-exported from `crux-types` should not be duplicated here
