---
crate: crux-types
test_strategy: inline
inline_test_modules: 7
dedicated_test_files: 1
test_areas:
  - module: crux_value
    coverage: "Crux<T> construction, serde roundtrip"
  - module: step
    coverage: "Step creation, status transitions, children"
  - module: id
    coverage: "CruxId/TaskId generation, uniqueness"
  - module: budget
    coverage: "Budget constructors, limit checking"
  - module: error
    coverage: "CruxErr display and serialization"
  - module: recovery
    coverage: "RecoveryKind serialization roundtrip"
  - module: testing
    coverage: "Test utility builders (behind test-utils)"
formal_verification: true
formal_verification_file: src/kani_proofs.rs
commands:
  default: "cargo nextest run -p crux-types"
  test_utils: "cargo nextest run -p crux-types --features test-utils"
---

# Testing: crux-types

## Test Strategy

7 inline `#[cfg(test)]` modules plus 1 dedicated test file. Tests focus
on serialization roundtrips and type invariants.

## Running

```bash
cargo nextest run -p crux-types
cargo nextest run -p crux-types --features test-utils
```

## Kani Proofs

Formal verification in `kani_proofs.rs` for critical invariants.
