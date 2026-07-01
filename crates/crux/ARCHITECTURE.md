---
crate: crux
pattern: facade
owns_logic: false
delegates_to:
  - crux-macros
  - crux-runtime
  - crux-script
integration_tests:
  - tests/agent_macro.rs
  - tests/combinators.rs
  - tests/delegation.rs
  - tests/speculation.rs
  - tests/task_registry.rs
---

# Architecture: crux

Thin facade crate. No logic of its own.

## Dependency Graph

```
crux
 +-- crux-macros (proc macros: agent, harness, evolve)
 +-- crux-runtime (all domain logic, re-exported via `pub use *`)
 +-- crux-script (optional, behind `script` feature)
```

## Design Rationale

Consumers add a single `crux` dependency to get macros and runtime
together. The facade pattern keeps the proc-macro crate (`crux-macros`)
separate from the runtime, as required by Rust's proc-macro compilation
model.
