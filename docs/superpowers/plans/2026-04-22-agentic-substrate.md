# Agentic Substrate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform crux from a single-runtime library into a general-purpose agentic execution
substrate by adding three orthogonal layers: a `Planner` port for abstract action dispatch,
a pure-domain `cruxx-domain` crate (no tokio/LLM deps), and an EDDOS-style typed event pipeline.

**Architecture:**
- `cruxx-domain` — new crate, zero async/LLM deps, contains `Planner` trait + `Action` enum +
  domain types. `cruxx-core` depends on it; external consumers (minibox, slash) can depend on
  it without pulling tokio.
- `Planner` port sits between `CruxCtx` and execution — `ctx.step()` goes through
  `Planner::next_action()` before executing, enabling dry-run, simulation, and gating.
- `EventPipeline` in `cruxx-core` — MPSC sender on `StepRecorder`, broadcast receiver for
  consumers; replaces the ad-hoc `events: Vec<Value>` field with a live typed stream.

**Tech Stack:** Rust 2024, tokio (broadcast/mpsc), serde, no new external deps.

---

## Scope check

Three independent subsystems. Each produces working, testable software on its own:

1. **Task A–C**: `cruxx-domain` crate — pure domain split
2. **Task D–F**: `Planner` port — action dispatch abstraction
3. **Task G–J**: EDDOS event pipeline — typed step event stream

Implement in order: domain split first (others depend on it having no-tokio types), then
Planner (uses domain types), then EDDOS (uses Planner + recorder).

---

## File map

### New files

| File | Responsibility |
|------|---------------|
| `crates/cruxx-domain/Cargo.toml` | New crate, no tokio dep |
| `crates/cruxx-domain/src/lib.rs` | Re-exports all domain items |
| `crates/cruxx-domain/src/action.rs` | `Action` enum — abstract step intents |
| `crates/cruxx-domain/src/planner.rs` | `Planner` trait (sync, no async) |
| `crates/cruxx-domain/src/plan_result.rs` | `PlanResult` — verdict from planner |
| `crates/cruxx-domain/src/event.rs` | `StepEvent` typed enum (replaces `Vec<Value>`) |
| `crates/cruxx-domain/src/pipeline.rs` | `EventPipeline` — MPSC + broadcast wiring |
| `crates/cruxx-core/src/planner_gate.rs` | `PlannerGate` adapter — wires Planner into CruxCtx |
| `crates/cruxx-core/src/event_sink.rs` | `EventSink` port — `StepRecorder` emits to it |

### Modified files

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Add `cruxx-domain` to members |
| `crates/cruxx-core/Cargo.toml` | Add `cruxx-domain` dep; add `event-pipeline` feature |
| `crates/cruxx-core/src/recorder.rs` | Inject optional `EventSink`; emit on record |
| `crates/cruxx-core/src/ctx.rs` | Accept optional `Planner`; call gate before step exec |
| `crates/cruxx-core/src/lib.rs` | Re-export new modules from prelude |
| `crates/cruxx/Cargo.toml` | Expose `cruxx-domain` dep + re-export |
| `crates/cruxx-types/src/step.rs` | Add `metadata: IndexMap<String, Value>` field |

---

## Task A: `cruxx-domain` crate scaffold

**Files:**
- Create: `crates/cruxx-domain/Cargo.toml`
- Create: `crates/cruxx-domain/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Write failing test for crate compilation**

  Create `crates/cruxx-domain/src/lib.rs` with just a marker test:

  ```rust
  #[cfg(test)]
  mod tests {
      #[test]
      fn domain_crate_compiles() {}
  }
  ```

- [ ] **Step 2: Create `crates/cruxx-domain/Cargo.toml`**

  ```toml
  [package]
  name = "cruxx-domain"
  description = "Pure domain types for the cruxx agentic DSL — no async, no LLM deps"
  version.workspace = true
  edition.workspace = true
  rust-version.workspace = true
  license.workspace = true
  authors.workspace = true
  repository.workspace = true
  homepage.workspace = true
  keywords.workspace = true
  categories.workspace = true

  [dependencies]
  serde = { workspace = true }
  serde_json = { workspace = true }
  thiserror = { workspace = true }
  cruxx-types = { path = "../cruxx-types", version = "0.2.5" }
  ```

- [ ] **Step 3: Register in workspace**

  Edit `Cargo.toml` — `members = ["crates/*"]` already matches; verify with:

  ```bash
  cargo build -p cruxx-domain
  ```

  Expected: compiles with zero warnings.

- [ ] **Step 4: Run test**

  ```bash
  cargo nextest run -p cruxx-domain
  ```

  Expected: 1 test passes.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/cruxx-domain Cargo.toml
  git commit -m "feat(domain): scaffold cruxx-domain crate"
  ```

---

## Task B: `Action` enum and `PlanResult`

**Files:**
- Create: `crates/cruxx-domain/src/action.rs`
- Create: `crates/cruxx-domain/src/plan_result.rs`
- Modify: `crates/cruxx-domain/src/lib.rs`

- [ ] **Step 1: Write failing tests**

  Add to `crates/cruxx-domain/src/lib.rs`:

  ```rust
  pub mod action;
  pub mod plan_result;

  #[cfg(test)]
  mod tests {
      use crate::action::{Action, StepIntent};
      use crate::plan_result::PlanResult;

      #[test]
      fn action_execute_roundtrips_serde() {
          let a = Action::Execute(StepIntent {
              name: "my_step".into(),
              priority: 0,
          });
          let json = serde_json::to_string(&a).unwrap();
          let back: Action = serde_json::from_str(&json).unwrap();
          assert!(matches!(back, Action::Execute(_)));
      }

      #[test]
      fn plan_result_allow_carries_action() {
          let a = Action::Execute(StepIntent { name: "x".into(), priority: 0 });
          let r = PlanResult::Allow(a.clone());
          assert!(matches!(r, PlanResult::Allow(_)));
      }

      #[test]
      fn plan_result_deny_carries_reason() {
          let r = PlanResult::Deny { reason: "unsafe".into() };
          if let PlanResult::Deny { reason } = r {
              assert_eq!(reason, "unsafe");
          }
      }

      #[test]
      fn plan_result_simulate_carries_output() {
          let r = PlanResult::Simulate { output: serde_json::json!(42) };
          if let PlanResult::Simulate { output } = r {
              assert_eq!(output, serde_json::json!(42));
          }
      }
  }
  ```

- [ ] **Step 2: Run test to verify failure**

  ```bash
  cargo nextest run -p cruxx-domain
  ```

  Expected: compile error — `action` and `plan_result` modules not found.

- [ ] **Step 3: Implement `action.rs`**

  ```rust
  //! Abstract step intents produced by a Planner.
  use serde::{Deserialize, Serialize};

  /// The name and scheduling priority of a step the planner permits.
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct StepIntent {
      /// Step name, matches the name passed to `ctx.step()`.
      pub name: String,
      /// Advisory scheduling priority (0 = normal, higher = prefer earlier).
      pub priority: u8,
  }

  /// Abstract action the planner emits for each step request.
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(tag = "kind", rename_all = "snake_case")]
  pub enum Action {
      /// Execute the step normally.
      Execute(StepIntent),
      /// Skip the step (record as Skipped).
      Skip { name: String },
      /// Finish the agent run immediately (budget exhausted or policy stop).
      Finish { reason: String },
  }

  impl Action {
      pub fn name(&self) -> &str {
          match self {
              Action::Execute(i) => &i.name,
              Action::Skip { name } => name,
              Action::Finish { .. } => "<finish>",
          }
      }
  }
  ```

- [ ] **Step 4: Implement `plan_result.rs`**

  ```rust
  //! Verdict returned by a Planner for a step request.
  use serde::{Deserialize, Serialize};
  use crate::action::Action;

  /// What the planner decided to do with a requested step.
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(tag = "verdict", rename_all = "snake_case")]
  pub enum PlanResult {
      /// Execute as requested. Contains the (possibly rewritten) action.
      Allow(Action),
      /// Block the step. Agent receives a `CruxErr::Denied` error.
      Deny { reason: String },
      /// Return a synthetic output without executing. Used for dry-run/simulation.
      Simulate { output: serde_json::Value },
  }
  ```

- [ ] **Step 5: Run tests — expect pass**

  ```bash
  cargo nextest run -p cruxx-domain
  ```

  Expected: 4 tests pass.

- [ ] **Step 6: Commit**

  ```bash
  git add crates/cruxx-domain/src/action.rs crates/cruxx-domain/src/plan_result.rs \
          crates/cruxx-domain/src/lib.rs
  git commit -m "feat(domain): add Action enum and PlanResult"
  ```

---

## Task C: `Planner` trait + `PassthroughPlanner`

**Files:**
- Create: `crates/cruxx-domain/src/planner.rs`
- Modify: `crates/cruxx-domain/src/lib.rs`

- [ ] **Step 1: Write failing tests**

  Add module declaration `pub mod planner;` to `lib.rs`, then add:

  ```rust
  // in lib.rs tests block
  use crate::planner::{PassthroughPlanner, Planner};

  #[test]
  fn passthrough_allows_all_steps() {
      let p = PassthroughPlanner;
      let result = p.next_action("my_step", 0);
      assert!(matches!(result, PlanResult::Allow(_)));
  }

  #[test]
  fn passthrough_preserves_step_name() {
      let p = PassthroughPlanner;
      if let PlanResult::Allow(action) = p.next_action("fetch_data", 0) {
          assert_eq!(action.name(), "fetch_data");
      } else {
          panic!("expected Allow");
      }
  }
  ```

- [ ] **Step 2: Run test to verify failure**

  ```bash
  cargo nextest run -p cruxx-domain
  ```

  Expected: compile error — `planner` module not found.

- [ ] **Step 3: Implement `planner.rs`**

  ```rust
  //! Planner port — decides what to do with each step request.
  //!
  //! The Planner trait is sync and stateless. Implementations can gate steps,
  //! rewrite priorities, simulate outputs, or stop execution entirely.
  //! CruxCtx calls `next_action` before executing each step.
  use crate::action::{Action, StepIntent};
  use crate::plan_result::PlanResult;

  /// Port: decides the fate of each step before execution.
  ///
  /// - Return `Allow` to execute normally (optionally with rewritten priority).
  /// - Return `Deny` to fail the step with a policy error.
  /// - Return `Simulate` to return a synthetic output without executing.
  pub trait Planner: Send + Sync + 'static {
      fn next_action(&self, step_name: &str, priority: u8) -> PlanResult;
  }

  /// Default planner: allows all steps through with unchanged priority.
  ///
  /// Used when no custom planner is attached to a `CruxCtx`.
  pub struct PassthroughPlanner;

  impl Planner for PassthroughPlanner {
      fn next_action(&self, step_name: &str, priority: u8) -> PlanResult {
          PlanResult::Allow(Action::Execute(StepIntent {
              name: step_name.to_string(),
              priority,
          }))
      }
  }

  /// Planner that denies all steps — useful as a dry-run sentinel in tests.
  pub struct DenyAllPlanner {
      pub reason: String,
  }

  impl Planner for DenyAllPlanner {
      fn next_action(&self, _name: &str, _priority: u8) -> PlanResult {
          PlanResult::Deny { reason: self.reason.clone() }
      }
  }

  /// Planner that simulates all steps with a fixed output value.
  pub struct SimulatePlanner {
      pub output: serde_json::Value,
  }

  impl Planner for SimulatePlanner {
      fn next_action(&self, _name: &str, _priority: u8) -> PlanResult {
          PlanResult::Simulate { output: self.output.clone() }
      }
  }
  ```

- [ ] **Step 4: Run tests**

  ```bash
  cargo nextest run -p cruxx-domain
  ```

  Expected: 6 tests pass.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/cruxx-domain/src/planner.rs crates/cruxx-domain/src/lib.rs
  git commit -m "feat(domain): add Planner trait with Passthrough/DenyAll/Simulate impls"
  ```

---

## Task D: Wire `Planner` into `CruxCtx`

**Files:**
- Create: `crates/cruxx-core/src/planner_gate.rs`
- Modify: `crates/cruxx-core/Cargo.toml`
- Modify: `crates/cruxx-core/src/ctx.rs`
- Modify: `crates/cruxx-core/src/lib.rs`

- [ ] **Step 1: Add `cruxx-domain` dep to `cruxx-core`**

  Edit `crates/cruxx-core/Cargo.toml`, add under `[dependencies]`:

  ```toml
  cruxx-domain = { path = "../cruxx-domain", version = "0.2.5" }
  ```

- [ ] **Step 2: Write failing tests in a new file**

  Create `crates/cruxx-core/src/planner_gate.rs`:

  ```rust
  //! PlannerGate — wires a Planner into step execution.
  #[cfg(test)]
  mod tests {
      use cruxx_domain::planner::{DenyAllPlanner, PassthroughPlanner, SimulatePlanner};
      use crate::ctx::CruxCtx;
      use crate::context::Context as _;
      use crate::types::error::CruxErr;

      #[tokio::test]
      async fn passthrough_planner_executes_step() {
          let mut ctx = CruxCtx::new("agent");
          ctx.set_planner(Box::new(PassthroughPlanner));
          let result = ctx.step("a", || async { Ok::<i32, CruxErr>(1) }).await;
          assert_eq!(result.unwrap(), 1);
      }

      #[tokio::test]
      async fn deny_planner_fails_step_with_denied_error() {
          let mut ctx = CruxCtx::new("agent");
          ctx.set_planner(Box::new(DenyAllPlanner { reason: "blocked".into() }));
          let result = ctx.step("a", || async { Ok::<i32, CruxErr>(1) }).await;
          assert!(result.is_err());
          let err = result.unwrap_err();
          assert!(err.to_string().contains("blocked"), "expected 'blocked' in: {err}");
      }

      #[tokio::test]
      async fn simulate_planner_returns_synthetic_output_without_running_closure() {
          use std::sync::Arc;
          use std::sync::atomic::{AtomicBool, Ordering};

          let ran = Arc::new(AtomicBool::new(false));
          let ran2 = ran.clone();

          let mut ctx = CruxCtx::new("agent");
          ctx.set_planner(Box::new(SimulatePlanner { output: serde_json::json!(99) }));

          let result = ctx
              .step("a", || async move {
                  ran2.store(true, Ordering::SeqCst);
                  Ok::<i32, CruxErr>(1)
              })
              .await;

          assert!(!ran.load(Ordering::SeqCst), "closure should not have run");
          assert_eq!(result.unwrap(), 99i32);
      }
  }
  ```

- [ ] **Step 3: Run tests to verify failure**

  ```bash
  cargo nextest run -p cruxx-core -- planner_gate
  ```

  Expected: compile error — `set_planner` not found on `CruxCtx`.

- [ ] **Step 4: Add `CruxErr::Denied` variant**

  Edit `crates/cruxx-types/src/error.rs`. Find the `CruxErr` enum and add:

  ```rust
  /// A planner denied this step.
  #[error("step '{step}' denied by planner: {reason}")]
  Denied { step: String, reason: String },
  ```

- [ ] **Step 5: Add `set_planner` and planner dispatch to `CruxCtx`**

  In `crates/cruxx-core/src/ctx.rs`:

  1. Add import at top:
     ```rust
     use cruxx_domain::planner::{PassthroughPlanner, Planner};
     use cruxx_domain::plan_result::PlanResult;
     ```

  2. Add field to `CruxCtx` struct:
     ```rust
     planner: Box<dyn Planner>,
     ```

  3. In `CruxCtx::new`, initialise:
     ```rust
     planner: Box::new(PassthroughPlanner),
     ```

  4. Add method:
     ```rust
     pub fn set_planner(&mut self, planner: Box<dyn Planner>) {
         self.planner = planner;
     }
     ```

  5. At the top of `step()` implementation (before executing the closure), add:

     ```rust
     let plan = self.planner.next_action(name, 0);
     match plan {
         PlanResult::Deny { reason } => {
             return Err(CruxErr::Denied {
                 step: name.to_string(),
                 reason,
             });
         }
         PlanResult::Simulate { output } => {
             return serde_json::from_value(output).map_err(|e| {
                 CruxErr::step_failed(name, &e.to_string())
             });
         }
         PlanResult::Allow(_) => {} // proceed normally
     }
     ```

  Apply the same `plan` check to `step_keyed` and `step_with_confidence` — copy the block
  verbatim at the start of each before any existing logic. `step_retryable` defers to
  `step_with_confidence`, so no additional change needed there.

- [ ] **Step 6: Add `planner_gate` module to `lib.rs`**

  In `crates/cruxx-core/src/lib.rs`, add:
  ```rust
  pub mod planner_gate;
  ```

- [ ] **Step 7: Run tests**

  ```bash
  cargo nextest run -p cruxx-core
  ```

  Expected: all tests pass including the 3 new planner_gate tests.

- [ ] **Step 8: Verify no regressions**

  ```bash
  cargo nextest run -p cruxx
  ```

  Expected: all integration tests pass.

- [ ] **Step 9: Commit**

  ```bash
  git add crates/cruxx-domain crates/cruxx-core/src/planner_gate.rs \
          crates/cruxx-core/src/ctx.rs crates/cruxx-core/Cargo.toml \
          crates/cruxx-types/src/error.rs crates/cruxx-core/src/lib.rs
  git commit -m "feat(planner): wire Planner port into CruxCtx step dispatch"
  ```

---

## Task E: `DelegationBuilder` + `SpeculationBuilder` planner propagation

**Files:**
- Modify: `crates/cruxx-core/src/delegation.rs`
- Modify: `crates/cruxx-core/src/speculation.rs`

Child contexts created by `delegate()` and `speculate()` should inherit the parent's planner.

- [ ] **Step 1: Write failing tests**

  Add to `crates/cruxx-core/src/delegation.rs` tests:

  ```rust
  #[tokio::test]
  async fn deny_planner_propagates_to_child_delegation() {
      use cruxx_domain::planner::DenyAllPlanner;

      let mut ctx = CruxCtx::new("parent");
      ctx.set_planner(Box::new(DenyAllPlanner { reason: "no-exec".into() }));

      let result = DelegationBuilder::<DoubleAgent>::new(&mut ctx, "child", 5)
          .run()
          .await;

      assert!(result.is_err());
  }
  ```

- [ ] **Step 2: Run to verify failure**

  ```bash
  cargo nextest run -p cruxx-core -- delegation
  ```

  Expected: test fails — planner not propagated, child runs freely.

- [ ] **Step 3: Propagate planner in `DelegationBuilder::run`**

  In `crates/cruxx-core/src/delegation.rs`, after `let mut child_ctx = CruxCtx::new(A::name());`,
  add planner propagation. This requires the child to receive a clone of the planner. The simplest
  approach is wrapping the planner in `Arc`:

  - Change `CruxCtx.planner` from `Box<dyn Planner>` to `Arc<dyn Planner>`.
  - Update `set_planner` to accept `Arc<dyn Planner>` (keep `Box` overload via `.into()`):

    ```rust
    pub fn set_planner(&mut self, planner: impl Planner + 'static) {
        self.planner = Arc::new(planner);
    }
    ```

  - In `DelegationBuilder::run`, after creating `child_ctx`:
    ```rust
    child_ctx.planner = Arc::clone(&self.ctx.planner);
    ```

  - In `SpeculationBuilder` wherever child contexts are created, apply the same clone.

- [ ] **Step 4: Run tests**

  ```bash
  cargo nextest run -p cruxx-core && cargo nextest run -p cruxx
  ```

  Expected: all pass.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/cruxx-core/src/delegation.rs crates/cruxx-core/src/speculation.rs \
          crates/cruxx-core/src/ctx.rs
  git commit -m "feat(planner): propagate planner Arc into child delegation/speculation contexts"
  ```

---

## Task F: Re-export Planner from `cruxx` facade and update prelude

**Files:**
- Modify: `crates/cruxx/Cargo.toml`
- Modify: `crates/cruxx-core/src/lib.rs` (prelude)

- [ ] **Step 1: Add `cruxx-domain` to `cruxx` facade**

  Edit `crates/cruxx/Cargo.toml`:

  ```toml
  [dependencies]
  cruxx-domain = { path = "../cruxx-domain", version = "0.2.5" }
  ```

- [ ] **Step 2: Add to prelude in `cruxx-core/src/lib.rs`**

  ```rust
  pub use cruxx_domain::planner::{DenyAllPlanner, PassthroughPlanner, Planner, SimulatePlanner};
  pub use cruxx_domain::plan_result::PlanResult;
  pub use cruxx_domain::action::{Action, StepIntent};
  ```

- [ ] **Step 3: Verify full workspace builds**

  ```bash
  cargo build --workspace && cargo nextest run --workspace
  ```

  Expected: zero errors, all tests pass.

- [ ] **Step 4: Commit**

  ```bash
  git add crates/cruxx/Cargo.toml crates/cruxx-core/src/lib.rs
  git commit -m "feat(planner): re-export Planner types from cruxx facade and prelude"
  ```

---

## Task G: `StepEvent` typed enum in `cruxx-domain`

**Files:**
- Create: `crates/cruxx-domain/src/event.rs`
- Modify: `crates/cruxx-domain/src/lib.rs`

`Step.events: Vec<Value>` is currently untyped. Replace with a typed `StepEvent` enum that
covers the standard step lifecycle and streaming payloads.

- [ ] **Step 1: Write failing tests**

  Add `pub mod event;` to `lib.rs`, then add to tests:

  ```rust
  use crate::event::StepEvent;

  #[test]
  fn step_event_serializes_tag() {
      let e = StepEvent::Started { step_name: "fetch".into() };
      let json = serde_json::to_value(&e).unwrap();
      assert_eq!(json["kind"], "started");
      assert_eq!(json["step_name"], "fetch");
  }

  #[test]
  fn step_event_chunk_carries_payload() {
      let e = StepEvent::Chunk { payload: serde_json::json!({"token": "hello"}) };
      let json = serde_json::to_value(&e).unwrap();
      assert_eq!(json["kind"], "chunk");
  }

  #[test]
  fn step_event_completed_carries_duration() {
      let e = StepEvent::Completed { step_name: "fetch".into(), duration_ms: 42 };
      let json = serde_json::to_value(&e).unwrap();
      assert_eq!(json["duration_ms"], 42);
  }
  ```

- [ ] **Step 2: Run to verify failure**

  ```bash
  cargo nextest run -p cruxx-domain
  ```

  Expected: compile error — `event` module not found.

- [ ] **Step 3: Implement `event.rs`**

  ```rust
  //! Typed step lifecycle and streaming events.
  //!
  //! These replace the untyped `events: Vec<serde_json::Value>` on `Step`.
  //! They are emitted by `StepRecorder` into the `EventPipeline` and can be
  //! consumed by observers without touching the trace directly.
  use serde::{Deserialize, Serialize};

  /// A typed event emitted during step execution.
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(tag = "kind", rename_all = "snake_case")]
  pub enum StepEvent {
      /// Step execution has started.
      Started { step_name: String },
      /// An intermediate streaming chunk from a streaming step.
      Chunk { payload: serde_json::Value },
      /// Step completed successfully.
      Completed { step_name: String, duration_ms: u64 },
      /// Step failed.
      Failed { step_name: String, error: String },
      /// Step was skipped by planner.
      Skipped { step_name: String, reason: String },
      /// Step was denied by planner.
      Denied { step_name: String, reason: String },
      /// Custom application event (escape hatch for domain-specific events).
      Custom { tag: String, payload: serde_json::Value },
  }
  ```

- [ ] **Step 4: Run tests**

  ```bash
  cargo nextest run -p cruxx-domain
  ```

  Expected: all tests pass.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/cruxx-domain/src/event.rs crates/cruxx-domain/src/lib.rs
  git commit -m "feat(events): add typed StepEvent enum to cruxx-domain"
  ```

---

## Task H: `EventPipeline` — MPSC + broadcast wiring

**Files:**
- Create: `crates/cruxx-domain/src/pipeline.rs`
- Modify: `crates/cruxx-domain/Cargo.toml` (add tokio dep, behind feature flag)
- Modify: `crates/cruxx-domain/src/lib.rs`

The pipeline lives in `cruxx-domain` behind a `tokio-pipeline` feature flag so no-tokio
consumers are unaffected.

- [ ] **Step 1: Add feature-gated tokio dep to `cruxx-domain`**

  Edit `crates/cruxx-domain/Cargo.toml`:

  ```toml
  [dependencies]
  # ... existing ...
  tokio = { workspace = true, optional = true, features = ["sync"] }

  [features]
  tokio-pipeline = ["dep:tokio"]
  ```

- [ ] **Step 2: Write failing tests**

  Add `#[cfg(feature = "tokio-pipeline")] pub mod pipeline;` to `lib.rs`, then add tests:

  ```rust
  #[cfg(feature = "tokio-pipeline")]
  mod pipeline_tests {
      use crate::event::StepEvent;
      use crate::pipeline::EventPipeline;

      #[tokio::test]
      async fn pipeline_delivers_event_to_subscriber() {
          let pipeline = EventPipeline::new(64);
          let mut rx = pipeline.subscribe();

          let sender = pipeline.sender();
          sender.send(StepEvent::Started { step_name: "test".into() }).ok();

          let received = rx.recv().await.unwrap();
          assert!(matches!(received, StepEvent::Started { .. }));
      }

      #[tokio::test]
      async fn pipeline_drops_events_when_no_subscriber() {
          let pipeline = EventPipeline::new(64);
          let sender = pipeline.sender();
          // Sending with no subscriber should not panic or block
          let _ = sender.send(StepEvent::Started { step_name: "x".into() });
      }

      #[tokio::test]
      async fn multiple_subscribers_each_receive_event() {
          let pipeline = EventPipeline::new(64);
          let mut rx1 = pipeline.subscribe();
          let mut rx2 = pipeline.subscribe();

          pipeline.sender()
              .send(StepEvent::Completed { step_name: "s".into(), duration_ms: 1 })
              .ok();

          assert!(matches!(rx1.recv().await.unwrap(), StepEvent::Completed { .. }));
          assert!(matches!(rx2.recv().await.unwrap(), StepEvent::Completed { .. }));
      }
  }
  ```

- [ ] **Step 3: Run to verify failure**

  ```bash
  cargo nextest run -p cruxx-domain --features tokio-pipeline
  ```

  Expected: compile error — `pipeline` module not found.

- [ ] **Step 4: Implement `pipeline.rs`**

  ```rust
  //! EventPipeline — MPSC ingestion → broadcast fan-out for step events.
  //!
  //! Architecture:
  //!   `EventSender` (cloneable) → tokio broadcast channel → `EventReceiver`s
  //!
  //! The broadcast channel gives each subscriber its own view of the event stream.
  //! Lagging receivers silently drop events (broadcast semantics) — consumers
  //! that need guaranteed delivery should use a separate MPSC tap.
  use tokio::sync::broadcast;
  use crate::event::StepEvent;

  /// A cloneable sender handle for emitting step events.
  pub type EventSender = broadcast::Sender<StepEvent>;

  /// A receiver handle for consuming step events.
  pub type EventReceiver = broadcast::Receiver<StepEvent>;

  /// The event pipeline — owns the broadcast channel.
  ///
  /// Call `subscribe()` before emitting events to avoid missing early events.
  pub struct EventPipeline {
      tx: broadcast::Sender<StepEvent>,
  }

  impl EventPipeline {
      /// Create a new pipeline with the given broadcast buffer capacity.
      ///
      /// `capacity` is the number of events buffered per subscriber. Lagging
      /// subscribers will miss events once the buffer fills.
      pub fn new(capacity: usize) -> Self {
          let (tx, _) = broadcast::channel(capacity);
          Self { tx }
      }

      /// Get a cloneable sender for emitting events.
      pub fn sender(&self) -> EventSender {
          self.tx.clone()
      }

      /// Subscribe to the event stream. Receives events emitted after this call.
      pub fn subscribe(&self) -> EventReceiver {
          self.tx.subscribe()
      }
  }
  ```

- [ ] **Step 5: Run tests**

  ```bash
  cargo nextest run -p cruxx-domain --features tokio-pipeline
  ```

  Expected: all tests pass.

- [ ] **Step 6: Commit**

  ```bash
  git add crates/cruxx-domain/src/pipeline.rs crates/cruxx-domain/src/lib.rs \
          crates/cruxx-domain/Cargo.toml
  git commit -m "feat(pipeline): add EventPipeline with broadcast fan-out to cruxx-domain"
  ```

---

## Task I: Wire `EventPipeline` into `StepRecorder` and `CruxCtx`

**Files:**
- Create: `crates/cruxx-core/src/event_sink.rs`
- Modify: `crates/cruxx-core/src/recorder.rs`
- Modify: `crates/cruxx-core/src/ctx.rs`
- Modify: `crates/cruxx-core/Cargo.toml`

- [ ] **Step 1: Enable `tokio-pipeline` feature in `cruxx-core`**

  Edit `crates/cruxx-core/Cargo.toml`:

  ```toml
  [dependencies]
  cruxx-domain = { path = "../cruxx-domain", version = "0.2.5",
                   features = ["tokio-pipeline"] }
  ```

- [ ] **Step 2: Write failing tests**

  Create `crates/cruxx-core/src/event_sink.rs`:

  ```rust
  //! EventSink — port for emitting step events from the recorder.
  #[cfg(test)]
  mod tests {
      use cruxx_domain::event::StepEvent;
      use cruxx_domain::pipeline::EventPipeline;
      use crate::ctx::CruxCtx;
      use crate::context::Context as _;
      use crate::types::error::CruxErr;

      #[tokio::test]
      async fn ctx_emits_started_event_on_step() {
          let pipeline = EventPipeline::new(64);
          let mut rx = pipeline.subscribe();

          let mut ctx = CruxCtx::new("agent");
          ctx.set_event_sender(pipeline.sender());

          ctx.step("my_step", || async { Ok::<i32, CruxErr>(1) })
              .await
              .unwrap();

          let ev = rx.recv().await.unwrap();
          assert!(matches!(ev, StepEvent::Started { ref step_name } if step_name == "my_step"),
              "expected Started, got: {ev:?}");
      }

      #[tokio::test]
      async fn ctx_emits_completed_event_after_ok_step() {
          let pipeline = EventPipeline::new(64);
          let mut rx = pipeline.subscribe();

          let mut ctx = CruxCtx::new("agent");
          ctx.set_event_sender(pipeline.sender());

          ctx.step("done_step", || async { Ok::<(), CruxErr>(()) })
              .await
              .unwrap();

          // Drain Started
          let _ = rx.recv().await.unwrap();
          let ev = rx.recv().await.unwrap();
          assert!(
              matches!(ev, StepEvent::Completed { ref step_name, .. } if step_name == "done_step"),
              "expected Completed, got: {ev:?}"
          );
      }

      #[tokio::test]
      async fn ctx_emits_failed_event_on_step_error() {
          let pipeline = EventPipeline::new(64);
          let mut rx = pipeline.subscribe();

          let mut ctx = CruxCtx::new("agent");
          ctx.set_event_sender(pipeline.sender());

          let _ = ctx
              .step("bad_step", || async { Err::<i32, _>(CruxErr::step_failed("bad_step", "boom")) })
              .await;

          // Drain Started
          let _ = rx.recv().await.unwrap();
          let ev = rx.recv().await.unwrap();
          assert!(
              matches!(ev, StepEvent::Failed { ref step_name, .. } if step_name == "bad_step"),
              "expected Failed, got: {ev:?}"
          );
      }
  }
  ```

- [ ] **Step 3: Run to verify failure**

  ```bash
  cargo nextest run -p cruxx-core -- event_sink
  ```

  Expected: compile error — `set_event_sender` not found.

- [ ] **Step 4: Add `EventSender` field to `CruxCtx`**

  In `crates/cruxx-core/src/ctx.rs`:

  1. Import:
     ```rust
     use cruxx_domain::pipeline::EventSender;
     use cruxx_domain::event::StepEvent;
     ```

  2. Add field to `CruxCtx`:
     ```rust
     event_sender: Option<EventSender>,
     ```

  3. Initialise in `new`:
     ```rust
     event_sender: None,
     ```

  4. Add method:
     ```rust
     pub fn set_event_sender(&mut self, sender: EventSender) {
         self.event_sender = Some(sender);
     }
     ```

  5. Add helper:
     ```rust
     fn emit(&self, event: StepEvent) {
         if let Some(ref tx) = self.event_sender {
             let _ = tx.send(event);
         }
     }
     ```

  6. In `step()` implementation, at the start (after planner check), add:
     ```rust
     self.emit(StepEvent::Started { step_name: name.to_string() });
     ```

     After the closure result, before returning, add:
     ```rust
     match &result {
         Ok(_) => self.emit(StepEvent::Completed {
             step_name: name.to_string(),
             duration_ms: rec.duration_ms,
         }),
         Err(e) => self.emit(StepEvent::Failed {
             step_name: name.to_string(),
             error: e.to_string(),
         }),
     }
     ```

     Apply `Started` + `Completed`/`Failed` to `step_keyed` and `step_with_confidence` as well.

- [ ] **Step 5: Add `event_sink` module to `lib.rs`**

  ```rust
  pub mod event_sink;
  ```

- [ ] **Step 6: Run tests**

  ```bash
  cargo nextest run -p cruxx-core
  ```

  Expected: all tests pass including the 3 new event_sink tests.

- [ ] **Step 7: Verify full workspace**

  ```bash
  cargo nextest run --workspace
  ```

  Expected: zero failures.

- [ ] **Step 8: Commit**

  ```bash
  git add crates/cruxx-core/src/event_sink.rs crates/cruxx-core/src/ctx.rs \
          crates/cruxx-core/Cargo.toml crates/cruxx-core/src/lib.rs
  git commit -m "feat(pipeline): wire EventPipeline sender into CruxCtx step emission"
  ```

---

## Task J: Add `metadata` field to `Step` and final integration test

**Files:**
- Modify: `crates/cruxx-types/src/step.rs`
- Create: `crates/cruxx/tests/substrate_integration.rs`

- [ ] **Step 1: Add `metadata` to `Step`**

  Edit `crates/cruxx-types/src/step.rs`:

  1. Add import:
     ```rust
     use std::collections::HashMap;
     ```

  2. Add field to `Step`:
     ```rust
     /// Arbitrary per-step metadata for extensibility.
     #[serde(default, skip_serializing_if = "HashMap::is_empty")]
     pub metadata: HashMap<String, serde_json::Value>,
     ```

  3. Update all `Step { .. }` constructors in `recorder.rs` to include `metadata: HashMap::new()`.

- [ ] **Step 2: Write integration test**

  Create `crates/cruxx/tests/substrate_integration.rs`:

  ```rust
  //! End-to-end test: Planner + EventPipeline together as the agentic substrate.
  use cruxx::prelude::*;
  use cruxx_domain::pipeline::EventPipeline;
  use cruxx_domain::event::StepEvent;
  use cruxx_domain::planner::{DenyAllPlanner, SimulatePlanner};

  #[tokio::test]
  async fn deny_planner_blocks_all_steps_end_to_end() {
      let mut ctx = CruxCtx::new("agent");
      ctx.set_planner(DenyAllPlanner { reason: "policy".into() });

      let result = ctx
          .step("fetch", || async { Ok::<i32, CruxErr>(1) })
          .await;

      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("policy"));
  }

  #[tokio::test]
  async fn simulate_planner_returns_value_without_side_effects() {
      let mut ctx = CruxCtx::new("agent");
      ctx.set_planner(SimulatePlanner { output: serde_json::json!(42) });

      let result = ctx
          .step("expensive_step", || async {
              panic!("should not run");
              #[allow(unreachable_code)]
              Ok::<i32, CruxErr>(0)
          })
          .await;

      assert_eq!(result.unwrap(), 42i32);
  }

  #[tokio::test]
  async fn event_pipeline_receives_all_step_lifecycle_events() {
      let pipeline = EventPipeline::new(128);
      let mut rx = pipeline.subscribe();

      let mut ctx = CruxCtx::new("agent");
      ctx.set_event_sender(pipeline.sender());

      ctx.step("step_a", || async { Ok::<i32, CruxErr>(1) }).await.unwrap();
      ctx.step("step_b", || async { Ok::<i32, CruxErr>(2) }).await.unwrap();

      let events: Vec<StepEvent> = (0..4).map(|_| rx.try_recv().unwrap()).collect();
      let kinds: Vec<&str> = events.iter().map(|e| match e {
          StepEvent::Started { .. } => "started",
          StepEvent::Completed { .. } => "completed",
          _ => "other",
      }).collect();

      assert_eq!(kinds, ["started", "completed", "started", "completed"]);
  }

  #[tokio::test]
  async fn planner_and_pipeline_compose() {
      // Passthrough planner + event pipeline work together
      let pipeline = EventPipeline::new(64);
      let mut rx = pipeline.subscribe();

      let mut ctx = CruxCtx::new("agent");
      ctx.set_event_sender(pipeline.sender());
      // Default passthrough planner — no set_planner call needed

      let result = ctx
          .step("compute", || async { Ok::<String, CruxErr>("done".into()) })
          .await;

      assert!(result.is_ok());
      let ev = rx.recv().await.unwrap();
      assert!(matches!(ev, StepEvent::Started { .. }));
  }
  ```

- [ ] **Step 3: Run integration tests**

  ```bash
  cargo nextest run -p cruxx -- substrate_integration
  ```

  Expected: all 4 tests pass.

- [ ] **Step 4: Full workspace gate**

  ```bash
  cargo clippy --workspace --all-targets -- -D warnings
  cargo nextest run --workspace
  ```

  Expected: zero clippy warnings, all tests pass.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/cruxx-types/src/step.rs crates/cruxx-core/src/recorder.rs \
          crates/cruxx/tests/substrate_integration.rs
  git commit -m "feat(substrate): add Step.metadata field and end-to-end substrate integration tests"
  ```

---

## Self-Review

### Spec coverage

| Feature | Tasks |
|---------|-------|
| Pure-domain `cruxx-domain` crate, no tokio/LLM | A |
| `Action` enum — abstract step intents | B |
| `PlanResult` — Allow/Deny/Simulate | B |
| `Planner` trait + PassthroughPlanner | C |
| DenyAllPlanner + SimulatePlanner | C |
| Planner wired into `CruxCtx.step()` | D |
| Planner propagates to child delegation/speculation | E |
| Re-exported from facade + prelude | F |
| Typed `StepEvent` enum | G |
| `EventPipeline` MPSC+broadcast | H |
| `CruxCtx` emits events on every step | I |
| `Step.metadata` extensibility field | J |
| End-to-end integration test | J |

### Placeholder scan

No TBDs, TODOs, or "similar to Task N" patterns. All code blocks are complete.

### Type consistency

- `PlanResult::Allow(Action)` — used consistently B→D→E→F
- `EventSender` type alias from `pipeline.rs` — used in `ctx.rs` and `event_sink.rs`
- `StepEvent` from `event.rs` — used in `pipeline.rs`, `ctx.rs`, integration tests
- `CruxCtx::set_planner` takes `impl Planner + 'static` after Task E Arc refactor
- `CruxCtx::set_event_sender` takes `EventSender` (broadcast::Sender clone)

All consistent.
