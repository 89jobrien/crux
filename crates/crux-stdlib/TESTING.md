---
crate: crux-stdlib
test_strategy: mixed
inline_test_modules: 2
dedicated_test_files: 5
test_areas:
  - module: fs
    coverage: "File read/write/copy/delete operations"
  - module: git
    coverage: "Git command execution and output parsing"
  - module: json
    coverage: "JSON transforms, merge, extraction"
  - module: text
    coverage: "Regex matching, splitting, parsing"
  - module: shell
    coverage: "Command execution and output capture"
  - module: ctrl
    coverage: "Conditional logic, loops, parallel dispatch"
commands:
  default: "cargo nextest run -p crux-stdlib"
---

# Testing: crux-stdlib

## Test Strategy

2 inline test modules + 5 dedicated test files covering each handler
domain.

## Running

```bash
cargo nextest run -p crux-stdlib
```
