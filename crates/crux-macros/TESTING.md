---
crate: crux-macros
test_strategy: downstream
inline_test_modules: 0
dedicated_test_files: 0
tested_via:
  - crate: crux
    file: tests/agent_macro.rs
    coverage: "End-to-end macro expansion"
commands:
  default: "cargo nextest run -p crux -- agent_macro"
notes:
  - "Proc-macro crates cannot have inline unit tests that invoke macros"
  - "Macro expansion issues surface as compile errors in dependent crates"
---

# Testing: crux-macros

## Test Strategy

Proc-macro crates cannot have inline unit tests that invoke the macros
(compilation model constraint). Testing is done through integration
tests in the `crux` facade crate.

## Running

```bash
cargo nextest run -p crux -- agent_macro
```
