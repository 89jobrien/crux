# cruxx-types Extraction Plan

**status: done**

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps
> use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract serializable types from `cruxx-core` into a new
`cruxx-types` crate with minimal dependencies (serde, chrono, ulid),
enabling downstream consumers (minibox-agent, minibox-core) to depend
on wire-format types without pulling the full cruxx runtime.

**Architecture:** `cruxx-types` contains only data types + serde impls.
`cruxx-core` re-exports everything from `cruxx-types` — no breaking
change for existing consumers. Published crates (`cruxai-core`,
`crux-agentic`) update to depend on `cruxx-types` instead of inlining.

**Tech Stack:** Rust 2024, serde, chrono, ulid

**Motivation:** minibox needs `Crux<T>`, `Step`, `Budget`, `CruxId`
etc. for pipeline trace storage and protocol types, but cannot depend
on the full `cruxx-core` runtime (which pulls tokio, LLM providers,
registry, etc.). See minibox spec:
`~/dev/minibox/docs/superpowers/specs/2026-04-20-crux-maestro-integration-design.md`

---

## Task 1: Create cruxx-types crate skeleton

**Files:**
- Create: `crates/cruxx-types/Cargo.toml`
- Create: `crates/cruxx-types/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Create crate directory**

```bash
mkdir -p crates/cruxx-types/src
```

- [ ] **Step 2: Write Cargo.toml**

```toml
[package]
name = "cruxx-types"
version = "0.1.0"
edition = "2024"
license.workspace = true
description = "Serializable wire-format types for the cruxx agentic DSL"

[dependencies]
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
chrono = { workspace = true, features = ["serde"] }
ulid = { workspace = true, features = ["serde"] }
```

- [ ] **Step 3: Write lib.rs stub**

```rust
//! cruxx-types: Serializable wire-format types for cruxx.
//!
//! This crate contains only data types with serde implementations.
//! It has no runtime, no async, no LLM dependencies. Designed for
//! cross-workspace consumption (e.g., minibox trace storage).

pub mod budget;
pub mod crux_value;
pub mod error;
pub mod id;
pub mod step;
pub mod recovery;
```

- [ ] **Step 4: Add to workspace members**

- [ ] **Step 5: `cargo check -p cruxx-types`**

- [ ] **Step 6: Commit**

```bash
git add crates/cruxx-types/ Cargo.toml
git commit -m "feat: scaffold cruxx-types crate"
```

---

## Task 2: Move type modules from cruxx-core

**Files:**
- Move: `crates/cruxx-core/src/types/budget.rs` → `crates/cruxx-types/src/budget.rs`
- Move: `crates/cruxx-core/src/types/crux_value.rs` → `crates/cruxx-types/src/crux_value.rs`
- Move: `crates/cruxx-core/src/types/error.rs` → `crates/cruxx-types/src/error.rs`
- Move: `crates/cruxx-core/src/types/id.rs` → `crates/cruxx-types/src/id.rs`
- Move: `crates/cruxx-core/src/types/step.rs` → `crates/cruxx-types/src/step.rs`
- Move: `crates/cruxx-core/src/types/recovery.rs` → `crates/cruxx-types/src/recovery.rs`
- Modify: `crates/cruxx-core/Cargo.toml` (add cruxx-types dep)
- Modify: `crates/cruxx-core/src/types/mod.rs` (re-export from cruxx-types)

- [ ] **Step 1: Copy files to cruxx-types**

Copy each type module. During the move, strip any imports that
reference cruxx-core internals (runtime, async, closures). The
`Recovery<T>` type has closure variants (`RetryWith`, `Escalate`) —
create a `RecoveryKind` enum in cruxx-types with only the serializable
subset:

```rust
/// Serializable subset of Recovery<T> — excludes closure variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryKind {
    Retry,
    Skip,
    Fail,
    Fallback,
}
```

Keep the full `Recovery<T>` (with closures) in `cruxx-core`.

- [ ] **Step 2: Fix imports in each moved file**

Each file should only import from `serde`, `chrono`, `ulid`,
`serde_json`, and sibling modules within `cruxx-types`. Remove any
`use crate::` paths that reference cruxx-core runtime modules.

- [ ] **Step 3: Add cruxx-types dependency to cruxx-core**

In `crates/cruxx-core/Cargo.toml`:
```toml
cruxx-types = { path = "../cruxx-types" }
```

- [ ] **Step 4: Re-export from cruxx-core types/mod.rs**

Replace the module declarations with re-exports:

```rust
// Re-export all wire-format types from cruxx-types.
pub use cruxx_types::budget;
pub use cruxx_types::crux_value;
pub use cruxx_types::error;
pub use cruxx_types::id;
pub use cruxx_types::step;
pub use cruxx_types::recovery::RecoveryKind;

// Keep closure-bearing Recovery<T> here (not in cruxx-types).
pub mod recovery;
```

- [ ] **Step 5: Verify no breaking changes**

```bash
cargo check --workspace
cargo test --workspace
```

All existing consumers of `cruxx_core::types::*` should still compile
unchanged via re-exports.

- [ ] **Step 6: Commit**

```bash
git commit -m "refactor: extract wire-format types into cruxx-types crate"
```

---

## Task 3: Update published crates to depend on cruxx-types

**Files:**
- Modify: `crates/cruxx-agentic/Cargo.toml` (if it inlines types)
- Modify: `crates/cruxx-model/Cargo.toml` (if it inlines types)

- [ ] **Step 1: Audit which crates duplicate type definitions**

Check `cruxai-core` and `crux-agentic` (published crates) for inlined
copies of `Crux<T>`, `Step`, `Budget`, etc.

- [ ] **Step 2: Replace with cruxx-types dependency**

For each crate that inlines types, replace with:
```toml
cruxx-types = { path = "../cruxx-types" }
```
And update imports.

- [ ] **Step 3: Verify**

```bash
cargo check --workspace
cargo test --workspace
```

- [ ] **Step 4: Commit**

```bash
git commit -m "refactor: published crates depend on cruxx-types"
```

---

## Task 4: Publish cruxx-types (or pin for minibox)

- [ ] **Step 1: Decide publication strategy**

Option A: Publish to crates.io (version 0.1.0)
Option B: Minibox uses git dependency with pinned rev

For now, prefer Option B until the API stabilizes:
```toml
# In minibox workspace Cargo.toml:
cruxx-types = { git = "...", rev = "abc123" }
```

- [ ] **Step 2: Tag the commit**

```bash
git tag cruxx-types-v0.1.0
```

- [ ] **Step 3: Update minibox to use the dependency**

In minibox's `Cargo.toml`, add:
```toml
cruxx-types = { git = "<crux-repo-url>", rev = "<sha>" }
```

---

## Dependency Graph

```
Task 1 (scaffold) → Task 2 (move types) → Task 3 (update consumers) → Task 4 (publish/pin)
```

All tasks are sequential. Task 2 is the highest-risk (most files
changed, potential import breakage).

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|------------|
| Type has hidden dep on cruxx-core runtime | Compile error in cruxx-types | Audit imports before move; stub or remove |
| Recovery<T> closure variants leak into wire types | Serde fails on closures | Explicit RecoveryKind subset, keep Recovery<T> in core |
| Re-export path change breaks downstream | Semver violation | Re-export preserves all existing paths |
| cruxx-plugin/cruxx-script need types too | Additional dep updates | They already depend on cruxx-core which re-exports |
