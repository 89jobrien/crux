//! Formal verification proofs for crux trace invariants.
//!
//! This crate contains no runtime code. It exists solely to host
//! [`hooo`]-backed `.hooo` proof files that verify structural
//! properties of the crux type system.
//!
//! # Feature flags
//!
//! - **`verify`** — enables the proof-checker integration test.
//!   Gate is off by default so ordinary `cargo test` stays fast.
//!   Run the proofs with:
//!   ```text
//!   cargo nextest run -p crux-verify --features verify
//!   ```
//!
//! # Proofs
//!
//! | File | Property |
//! |------|----------|
//! | `proofs/budget.hooo` | `BudgetTracker::remaining()` is non-increasing after any operation |
//!
//! # Proof strategy
//!
//! Each proof file axiomatises the relevant single-step invariant
//! (e.g. *consume reduces remaining*) and then derives multi-step
//! consequences via transitivity.  The axioms act as a trusted base
//! that mirrors the Rust implementation; the theorems are mechanically
//! verified by the hooo checker.
