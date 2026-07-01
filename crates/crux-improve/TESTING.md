---
crate: crux-improve
test_strategy: inline
inline_test_modules: 1
dedicated_test_files: 0
test_areas:
  - module: lib
    coverage: "Type construction and serialization"
commands:
  default: "cargo nextest run -p crux-improve"
---

# Testing: crux-improve

## Test Strategy

1 inline `#[cfg(test)]` module covering type construction and
serialization.

## Running

```bash
cargo nextest run -p crux-improve
```
