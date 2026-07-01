---
crate: crux-runtime
test_strategy: inline
inline_test_modules: 25
dedicated_test_files: 0
test_areas:
  - module: ctx
    coverage: "Combinator behavior, step recording"
  - module: delegation
    coverage: "Budget propagation, child context creation"
  - module: speculation
    coverage: "Racing, pick_best_by, rejection marking"
  - module: hooks
    coverage: "Hook registration, dispatch, Recovery handling"
  - module: recorder
    coverage: "Step append, trace structure"
  - module: replay
    coverage: "Strict/lenient matching, hash identity"
  - module: registry
    coverage: "Backend trait, CAS, status transitions"
  - module: safety
    coverage: "SafetyPolicy trait dispatch"
  - module: approval
    coverage: "ApprovalGate integration"
  - module: governance
    coverage: "Policy composition"
  - module: trust
    coverage: "TrustScore tracking"
  - module: audit
    coverage: "AuditSink event recording"
formal_verification: true
formal_verification_file: src/kani_proofs.rs
commands:
  default: "cargo nextest run -p crux-runtime"
  redb: "cargo nextest run -p crux-runtime --features redb"
  kani: "cargo kani"
---

# Testing: crux-runtime

## Test Strategy

25 inline `#[cfg(test)]` modules — each module tests its own logic in
isolation.

## Running

```bash
cargo nextest run -p crux-runtime
cargo nextest run -p crux-runtime --features redb
```

## Kani Proofs

Formal verification proofs in `kani_proofs.rs` (behind `#[cfg(kani)]`).
Run with `cargo kani` if the Kani verifier is installed.
