---
crate: crux-macros
role: proc-macro
key_entry_points:
  - path: "src/agent.rs"
    purpose: "agent macro expansion"
  - path: "src/parse.rs"
    purpose: "Shared attribute parsing"
howto:
  - task: "Add a new macro option"
    steps:
      - "Add parsing in parse.rs"
      - "Handle option in relevant expand() function"
      - "Add integration test in crates/crux/tests/"
  - task: "Debug expansion"
    command: "cargo expand -p crux --test agent_macro"
pitfalls:
  - "Cannot import runtime types directly (proc-macro boundary)"
  - "Generated code references crux_runtime:: paths — keep stable"
  - "Test via integration tests in crates/crux/tests/agent_macro.rs"
---

# Agents: crux-macros

## For AI Agents Working With This Crate

Proc-macro crate. Changes here affect all downstream `#[crux::agent]`,
`#[crux::harness]`, and `#[crux::evolve]` call sites.

### Key Constraints

- Cannot import runtime types directly (proc-macro compilation boundary)
- Generated code references `crux_runtime::` paths — keep these stable
- Test via integration tests in `crates/crux/tests/agent_macro.rs`

### Debugging Expansion

```bash
cargo expand -p crux --test agent_macro
```
