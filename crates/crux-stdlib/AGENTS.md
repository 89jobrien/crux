---
crate: crux-stdlib
role: stdlib-handlers
constraints:
  - "Handlers must be deterministic"
  - "No LLM or network dependencies"
  - "Side effects limited to filesystem and shell"
  - "Each handler independently testable"
howto:
  - task: "Add a new handler"
    steps:
      - "Add to appropriate domain module (fs, git, json, text, shell, ctrl)"
      - "Register in register_all() in lib.rs"
      - "Add tests in #[cfg(test)] or tests/"
do_not_add:
  - "LLM calls — those go in crux-agentic"
  - "Network requests — those go in crux-agentic"
---

# Agents: crux-stdlib

## For AI Agents Working With This Crate

Standard library of deterministic handlers. No LLM calls, no network
requests (except shell commands the user defines).

### Design Rules

- Handlers must be deterministic given the same input
- Side effects are limited to filesystem and shell
- No LLM or network dependencies — those go in `crux-agentic`
- Each handler should be independently testable
