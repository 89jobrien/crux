---
crate: crux-stdlib
pattern: flat-handler-library
entry_point: "register_all()"
design_principles:
  - "All handlers deterministic (no LLM calls)"
  - "Side effects limited to filesystem and shell"
  - "Each handler independently testable"
modules:
  - name: fs
    purpose: "Filesystem operations"
  - name: git
    purpose: "Git commands"
  - name: json
    purpose: "JSON manipulation"
  - name: text
    purpose: "Text parsing and regex"
  - name: shell
    purpose: "Shell command execution"
  - name: ctrl
    purpose: "Control flow (if/else, loop, parallel)"
  - name: error
    purpose: "Shared error types"
---

# Architecture: crux-stdlib

Flat handler library. Each module corresponds to a handler domain and
registers its handlers into `HandlerRegistry`.

## Design Principles

- All handlers are deterministic (no LLM calls)
- Side effects are limited to filesystem and shell
- Each handler is independently testable
- `register_all()` is the single registration entry point
