---
crate: crux-domain
role: pure-domain
constraints:
  - "No tokio, no async, no LLM deps, no network calls"
  - "All types must be Send + Sync"
howto:
  - task: "Add a new Planner"
    steps:
      - "Implement Planner trait"
      - "Add to planner.rs"
      - "Re-export from crux-runtime::prelude if widely used"
      - "Add unit tests in #[cfg(test)] module"
---

# Agents: crux-domain

## For AI Agents Working With This Crate

Pure domain types. No infrastructure dependencies allowed.

### Rules

- No `tokio`, no async, no LLM deps, no network calls
- All types must be `Send + Sync`
- New planner implementations go here
- Keep this crate as the lightweight domain core
