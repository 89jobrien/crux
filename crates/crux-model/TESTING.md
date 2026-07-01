---
crate: crux-model
test_strategy: inline
inline_test_modules: 9
dedicated_test_files: 0
test_areas:
  - module: parser
    coverage: "Model string parsing across providers"
  - module: vendor
    coverage: "Vendor detection from string patterns"
  - module: canonical
    coverage: "Normalization and equality"
  - module: provider_ref
    coverage: "Metadata attachment"
  - module: error
    coverage: "Parse error variants"
commands:
  default: "cargo nextest run -p crux-model"
---

# Testing: crux-model

## Test Strategy

9 inline `#[cfg(test)]` modules — one per source file. Tests cover
parsing, vendor detection, canonicalization, and error cases.

## Running

```bash
cargo nextest run -p crux-model
```
