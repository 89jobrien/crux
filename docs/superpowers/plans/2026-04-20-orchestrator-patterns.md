# Orchestrator Patterns Implementation Plan

status: done

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add self-evolving container orchestration primitives to crux — harness profiles,
safety policies, approval gates, container/harness handlers, deterministic planning, and
macro support.

**Architecture:** Domain types and traits (ports) go in `crux-runtime`. Step handlers (adapters)
go in `crux-agentic`. Deterministic planner logic extends `crux-planner`. New proc macros
`#[crux::harness]` and `#[crux::evolve]` live in `crux-macros`. All domain logic is generic
over traits; no concrete adapters in core.

**Tech Stack:** Rust 2024, serde, tokio, thiserror, chrono, proc-macro2/syn/quote

---

## File Structure

### crux-runtime (domain types + traits)

| File                     | Responsibility                                              |
| ------------------------ | ----------------------------------------------------------- |
| `src/types/harness.rs`   | `HarnessProfile`, `ResourceHints`, `HarnessDiff`            |
| `src/types/evolution.rs` | `EvolutionOutcome` enum                                     |
| `src/safety.rs`          | `SafetyPolicy` trait, `SafetyViolation` error               |
| `src/approval.rs`        | `ApprovalGate` trait, `ApprovalRequest`, `ApprovalDecision` |

### crux-agentic (handlers/adapters)

| File                                | Responsibility                                     |
| ----------------------------------- | -------------------------------------------------- |
| `src/container.rs`                  | `container::run`, `container::wait` step handlers  |
| `src/harness.rs`                    | `harness::evolve`, `harness::canary` step handlers |
| `src/adapters/container_client.rs`  | `ContainerClient` trait + mock impl                |
| `src/adapters/terminal_approval.rs` | Terminal stdin approval gate adapter               |

### crux-planner (deterministic planning)

| File               | Responsibility                                           |
| ------------------ | -------------------------------------------------------- |
| `src/evolution.rs` | `EvolutionPlanner` — metrics-to-diff pipeline generation |
| `src/metrics.rs`   | `MetricsAggregator` trait, `RunMetrics` type             |

### crux-macros (proc macros)

| File             | Responsibility               |
| ---------------- | ---------------------------- |
| `src/harness.rs` | `#[crux::harness]` expansion |
| `src/evolve.rs`  | `#[crux::evolve]` expansion  |

---

## Task 1: Domain Types — HarnessProfile and ResourceHints

**Files:**

- Create: `crates/crux-runtime/src/types/harness.rs`
- Modify: `crates/crux-runtime/src/types/mod.rs`
- Modify: `crates/crux-runtime/src/lib.rs` (re-export in prelude)

- [ ] **Step 1: Write the failing test**

```rust
// crates/crux-runtime/src/types/harness.rs (at bottom, #[cfg(test)] mod tests)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_profile_serde_round_trip() {
        let profile = HarnessProfile {
            id: "default-v1".to_string(),
            resources: ResourceHints {
                memory_mb: 512,
                cpu_millicores: 1000,
                timeout_seconds: 300,
            },
            network_access: false,
            allowed_syscalls: vec!["read".into(), "write".into(), "mmap".into()],
        };
        let json = serde_json::to_string(&profile).unwrap();
        let back: HarnessProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "default-v1");
        assert_eq!(back.resources.memory_mb, 512);
        assert!(!back.network_access);
    }

    #[test]
    fn harness_diff_fields_changed() {
        let diff = HarnessDiff {
            memory_delta_mb: Some(256),
            cpu_delta_millicores: None,
            timeout_delta_seconds: Some(60),
            network_access_change: Some(true),
            syscall_additions: vec!["connect".into()],
            syscall_removals: vec![],
        };
        assert!(diff.has_changes());
    }

    #[test]
    fn empty_diff_has_no_changes() {
        let diff = HarnessDiff::default();
        assert!(!diff.has_changes());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p crux-runtime harness`
Expected: compilation error — module not found

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/crux-runtime/src/types/harness.rs
use serde::{Deserialize, Serialize};

/// Resource limits for a container execution environment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceHints {
    pub memory_mb: u64,
    pub cpu_millicores: u64,
    pub timeout_seconds: u64,
}

/// A named, versioned execution profile for container workloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessProfile {
    pub id: String,
    pub resources: ResourceHints,
    pub network_access: bool,
    pub allowed_syscalls: Vec<String>,
}

/// A proposed change to a HarnessProfile.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HarnessDiff {
    pub memory_delta_mb: Option<i64>,
    pub cpu_delta_millicores: Option<i64>,
    pub timeout_delta_seconds: Option<i64>,
    pub network_access_change: Option<bool>,
    pub syscall_additions: Vec<String>,
    pub syscall_removals: Vec<String>,
}

impl HarnessDiff {
    /// Returns true if any field contains a change.
    pub fn has_changes(&self) -> bool {
        self.memory_delta_mb.is_some()
            || self.cpu_delta_millicores.is_some()
            || self.timeout_delta_seconds.is_some()
            || self.network_access_change.is_some()
            || !self.syscall_additions.is_empty()
            || !self.syscall_removals.is_empty()
    }

    /// Apply this diff to a profile, producing a new profile.
    pub fn apply(&self, base: &HarnessProfile) -> HarnessProfile {
        let mut result = base.clone();
        if let Some(delta) = self.memory_delta_mb {
            result.resources.memory_mb = (result.resources.memory_mb as i64 + delta).max(0) as u64;
        }
        if let Some(delta) = self.cpu_delta_millicores {
            result.resources.cpu_millicores =
                (result.resources.cpu_millicores as i64 + delta).max(0) as u64;
        }
        if let Some(delta) = self.timeout_delta_seconds {
            result.resources.timeout_seconds =
                (result.resources.timeout_seconds as i64 + delta).max(0) as u64;
        }
        if let Some(net) = self.network_access_change {
            result.network_access = net;
        }
        for syscall in &self.syscall_additions {
            if !result.allowed_syscalls.contains(syscall) {
                result.allowed_syscalls.push(syscall.clone());
            }
        }
        result.allowed_syscalls.retain(|s| !self.syscall_removals.contains(s));
        result
    }
}
```

- [ ] **Step 4: Wire up the module**

Add to `crates/crux-runtime/src/types/mod.rs`:

```rust
pub mod harness;
```

Add to `crates/crux-runtime/src/prelude` in `lib.rs`:

```rust
pub use crate::types::harness::{HarnessDiff, HarnessProfile, ResourceHints};
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo nextest run -p crux-runtime harness`
Expected: 3 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/crux-runtime/src/types/harness.rs crates/crux-runtime/src/types/mod.rs crates/crux-runtime/src/lib.rs
git commit -m "feat(core): add HarnessProfile, ResourceHints, and HarnessDiff types"
```

---

## Task 2: Domain Types — EvolutionOutcome

**Files:**

- Create: `crates/crux-runtime/src/types/evolution.rs`
- Modify: `crates/crux-runtime/src/types/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/crux-runtime/src/types/evolution.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_serde_round_trip() {
        let outcome = EvolutionOutcome::Promoted {
            profile_id: "evolved-v2".into(),
            improvement_pct: 15.3,
        };
        let json = serde_json::to_string(&outcome).unwrap();
        let back: EvolutionOutcome = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, EvolutionOutcome::Promoted { .. }));
    }

    #[test]
    fn discarded_carries_reason() {
        let outcome = EvolutionOutcome::Discarded {
            reason: "candidate 12% slower than baseline".into(),
        };
        if let EvolutionOutcome::Discarded { reason } = outcome {
            assert!(reason.contains("slower"));
        } else {
            panic!("expected Discarded");
        }
    }

    #[test]
    fn blocked_carries_violation() {
        let outcome = EvolutionOutcome::Blocked {
            violation: "memory exceeds hard cap (4096 MB)".into(),
        };
        assert!(matches!(outcome, EvolutionOutcome::Blocked { .. }));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p crux-runtime evolution`
Expected: compilation error

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/crux-runtime/src/types/evolution.rs
use serde::{Deserialize, Serialize};

/// The result of an evolution cycle — did the candidate get promoted?
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum EvolutionOutcome {
    /// Candidate beat baseline — new profile is now active.
    Promoted {
        profile_id: String,
        improvement_pct: f64,
    },
    /// Candidate did not beat baseline — discarded.
    Discarded { reason: String },
    /// Safety policy blocked the proposed diff.
    Blocked { violation: String },
    /// Approval gate rejected the escalation.
    Denied { request_summary: String },
}
```

- [ ] **Step 4: Wire module**

Add to `crates/crux-runtime/src/types/mod.rs`:

```rust
pub mod evolution;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo nextest run -p crux-runtime evolution`
Expected: 3 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/crux-runtime/src/types/evolution.rs crates/crux-runtime/src/types/mod.rs
git commit -m "feat(core): add EvolutionOutcome enum"
```

---

## Task 3: SafetyPolicy Trait

**Files:**

- Create: `crates/crux-runtime/src/safety.rs`
- Modify: `crates/crux-runtime/src/lib.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/crux-runtime/src/safety.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::harness::{HarnessDiff, HarnessProfile, ResourceHints};

    struct StrictPolicy {
        max_memory_mb: u64,
    }

    impl SafetyPolicy for StrictPolicy {
        fn validate(&self, diff: &HarnessDiff, base: &HarnessProfile) -> Result<(), SafetyViolation> {
            let proposed = diff.apply(base);
            if proposed.resources.memory_mb > self.max_memory_mb {
                return Err(SafetyViolation::HardCapExceeded {
                    resource: "memory_mb".into(),
                    limit: self.max_memory_mb,
                    proposed: proposed.resources.memory_mb,
                });
            }
            Ok(())
        }

        fn requires_approval(&self, diff: &HarnessDiff) -> bool {
            diff.network_access_change == Some(true) || !diff.syscall_additions.is_empty()
        }
    }

    fn test_profile() -> HarnessProfile {
        HarnessProfile {
            id: "test-v1".into(),
            resources: ResourceHints {
                memory_mb: 512,
                cpu_millicores: 1000,
                timeout_seconds: 300,
            },
            network_access: false,
            allowed_syscalls: vec!["read".into(), "write".into()],
        }
    }

    #[test]
    fn validate_passes_within_limits() {
        let policy = StrictPolicy { max_memory_mb: 2048 };
        let diff = HarnessDiff {
            memory_delta_mb: Some(256),
            ..Default::default()
        };
        assert!(policy.validate(&diff, &test_profile()).is_ok());
    }

    #[test]
    fn validate_fails_above_hard_cap() {
        let policy = StrictPolicy { max_memory_mb: 600 };
        let diff = HarnessDiff {
            memory_delta_mb: Some(256),
            ..Default::default()
        };
        let result = policy.validate(&diff, &test_profile());
        assert!(matches!(result, Err(SafetyViolation::HardCapExceeded { .. })));
    }

    #[test]
    fn requires_approval_for_network_escalation() {
        let policy = StrictPolicy { max_memory_mb: 4096 };
        let diff = HarnessDiff {
            network_access_change: Some(true),
            ..Default::default()
        };
        assert!(policy.requires_approval(&diff));
    }

    #[test]
    fn no_approval_for_resource_only_change() {
        let policy = StrictPolicy { max_memory_mb: 4096 };
        let diff = HarnessDiff {
            memory_delta_mb: Some(128),
            ..Default::default()
        };
        assert!(!policy.requires_approval(&diff));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p crux-runtime safety`
Expected: compilation error

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/crux-runtime/src/safety.rs
use crate::types::harness::{HarnessDiff, HarnessProfile};
use thiserror::Error;

/// Why a safety policy rejected a proposed diff.
#[derive(Debug, Clone, Error)]
pub enum SafetyViolation {
    #[error("hard cap exceeded: {resource} limit={limit}, proposed={proposed}")]
    HardCapExceeded {
        resource: String,
        limit: u64,
        proposed: u64,
    },
    #[error("forbidden syscall: {syscall}")]
    ForbiddenSyscall { syscall: String },
    #[error("policy violation: {reason}")]
    Custom { reason: String },
}

/// Port: validates proposed harness changes against safety constraints.
pub trait SafetyPolicy: Send + Sync {
    /// Check whether a diff is safe to apply against the given base profile.
    fn validate(&self, diff: &HarnessDiff, base: &HarnessProfile) -> Result<(), SafetyViolation>;

    /// Returns true if this diff requires human/gate approval before applying.
    fn requires_approval(&self, diff: &HarnessDiff) -> bool;
}
```

- [ ] **Step 4: Wire module**

Add to `crates/crux-runtime/src/lib.rs`:

```rust
pub mod safety;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo nextest run -p crux-runtime safety`
Expected: 4 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/crux-runtime/src/safety.rs crates/crux-runtime/src/lib.rs
git commit -m "feat(core): add SafetyPolicy trait and SafetyViolation error"
```

---

## Task 4: ApprovalGate Trait

**Files:**

- Create: `crates/crux-runtime/src/approval.rs`
- Modify: `crates/crux-runtime/src/lib.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/crux-runtime/src/approval.rs
#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysApprove;

    impl ApprovalGate for AlwaysApprove {
        async fn request_approval(&self, request: &ApprovalRequest) -> ApprovalDecision {
            let _ = request;
            ApprovalDecision::Approved
        }
    }

    struct AlwaysDeny;

    impl ApprovalGate for AlwaysDeny {
        async fn request_approval(&self, _request: &ApprovalRequest) -> ApprovalDecision {
            ApprovalDecision::Denied {
                reason: "policy".into(),
            }
        }
    }

    #[tokio::test]
    async fn approve_gate_returns_approved() {
        let gate = AlwaysApprove;
        let req = ApprovalRequest {
            summary: "enable network access".into(),
            diff_description: "network_access: false -> true".into(),
            risk_level: RiskLevel::Medium,
        };
        assert!(matches!(gate.request_approval(&req).await, ApprovalDecision::Approved));
    }

    #[tokio::test]
    async fn deny_gate_returns_denied() {
        let gate = AlwaysDeny;
        let req = ApprovalRequest {
            summary: "add dangerous syscall".into(),
            diff_description: "syscalls += ptrace".into(),
            risk_level: RiskLevel::High,
        };
        assert!(matches!(gate.request_approval(&req).await, ApprovalDecision::Denied { .. }));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p crux-runtime approval`
Expected: compilation error

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/crux-runtime/src/approval.rs
use serde::{Deserialize, Serialize};
use std::future::Future;

/// How risky a proposed change is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// A request sent to the approval gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub summary: String,
    pub diff_description: String,
    pub risk_level: RiskLevel,
}

/// The gate's response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum ApprovalDecision {
    Approved,
    Denied { reason: String },
    Deferred { timeout_seconds: u64 },
}

/// Port: gates escalation requests (human-in-the-loop or policy engine).
pub trait ApprovalGate: Send + Sync {
    fn request_approval(
        &self,
        request: &ApprovalRequest,
    ) -> impl Future<Output = ApprovalDecision> + Send;
}
```

- [ ] **Step 4: Wire module**

Add to `crates/crux-runtime/src/lib.rs`:

```rust
pub mod approval;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo nextest run -p crux-runtime approval`
Expected: 2 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/crux-runtime/src/approval.rs crates/crux-runtime/src/lib.rs
git commit -m "feat(core): add ApprovalGate trait with RiskLevel and ApprovalDecision"
```

---

## Task 5: Approval Hook in HookRegistry

**Files:**

- Modify: `crates/crux-runtime/src/hooks.rs`

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block in `hooks.rs`:

```rust
#[tokio::test]
async fn approval_fires_when_registered() {
    let mut hooks = HookRegistry::new();
    hooks.on_approval_required(|req| async move {
        let _ = req;
        Recovery::Continue
    });
    let request = serde_json::json!({"summary": "enable network"});
    let result = hooks.check_approval(request).await;
    assert!(result.is_some());
}

#[tokio::test]
async fn approval_returns_none_without_handler() {
    let hooks = HookRegistry::new();
    let request = serde_json::json!({"summary": "enable network"});
    assert!(hooks.check_approval(request).await.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p crux-runtime approval_fires`
Expected: compilation error — no method `on_approval_required` / `check_approval`

- [ ] **Step 3: Write minimal implementation**

Add a new handler type alias above the struct:

```rust
/// Boxed async handler for approval-required events.
type ApprovalHandler = Box<
    dyn Fn(serde_json::Value) -> Pin<Box<dyn Future<Output = Recovery<serde_json::Value>> + Send>>
        + Send
        + Sync,
>;
```

Add field to `HookRegistry`:

```rust
approval_handler: Option<ApprovalHandler>,
```

Initialize in `new()`:

```rust
approval_handler: None,
```

Add methods:

```rust
/// Register an approval-required handler. Fires when a step needs gate approval.
pub fn on_approval_required<F, Fut>(&mut self, handler: F)
where
    F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Recovery<serde_json::Value>> + Send + 'static,
{
    self.approval_handler = Some(Box::new(move |req| Box::pin(handler(req))));
}

/// Invoke the approval handler if registered.
pub async fn check_approval(
    &self,
    request: serde_json::Value,
) -> Option<Recovery<serde_json::Value>> {
    if let Some(handler) = &self.approval_handler {
        Some(handler(request).await)
    } else {
        None
    }
}
```

Update the `Debug` impl to include `has_approval_handler`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -p crux-runtime -- hooks`
Expected: all hooks tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/crux-runtime/src/hooks.rs
git commit -m "feat(core): add on_approval_required hook to HookRegistry"
```

---

## Task 6: Container Handlers in crux-agentic

**Files:**

- Create: `crates/crux-agentic/src/container.rs`
- Create: `crates/crux-agentic/src/adapters/container_client.rs`
- Modify: `crates/crux-agentic/src/adapters/mod.rs`
- Modify: `crates/crux-agentic/src/lib.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/crux-agentic/tests/container.rs
use crux_agentic::container;
use crux_script::HandlerRegistry;
use serde_json::json;

#[tokio::test]
async fn container_run_handler_registered() {
    let mut registry = HandlerRegistry::new();
    container::register(&mut registry);
    assert!(registry.get("container::run").is_some());
    assert!(registry.get("container::wait").is_some());
}

#[tokio::test]
async fn container_run_returns_container_id() {
    let mut registry = HandlerRegistry::new();
    container::register(&mut registry);
    let handler = registry.get("container::run").unwrap();
    let input = json!({
        "image": "alpine:latest",
        "cmd": ["echo", "hello"],
        "profile_id": "default-v1"
    });
    let result = handler.call(input).await.unwrap();
    // Mock client returns a deterministic container_id
    assert!(result.get("container_id").is_some());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p crux-agentic container`
Expected: compilation error — module not found

- [ ] **Step 3: Create ContainerClient trait (port)**

```rust
// crates/crux-agentic/src/adapters/container_client.rs
use serde::{Deserialize, Serialize};
use std::future::Future;

/// Status of a container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerState {
    Running,
    Stopped { exit_code: i32 },
    Failed { error: String },
}

/// Result of starting a container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerHandle {
    pub container_id: String,
    pub state: ContainerState,
}

/// Port: manages container lifecycle.
pub trait ContainerClient: Send + Sync {
    fn run(
        &self,
        image: &str,
        cmd: &[String],
        memory_mb: u64,
        cpu_millicores: u64,
        timeout_seconds: u64,
    ) -> impl Future<Output = Result<ContainerHandle, String>> + Send;

    fn wait(
        &self,
        container_id: &str,
        timeout_seconds: u64,
    ) -> impl Future<Output = Result<ContainerState, String>> + Send;
}

/// Mock implementation for testing.
pub struct MockContainerClient;

impl ContainerClient for MockContainerClient {
    async fn run(
        &self,
        _image: &str,
        _cmd: &[String],
        _memory_mb: u64,
        _cpu_millicores: u64,
        _timeout_seconds: u64,
    ) -> Result<ContainerHandle, String> {
        Ok(ContainerHandle {
            container_id: "mock-container-001".into(),
            state: ContainerState::Running,
        })
    }

    async fn wait(
        &self,
        _container_id: &str,
        _timeout_seconds: u64,
    ) -> Result<ContainerState, String> {
        Ok(ContainerState::Stopped { exit_code: 0 })
    }
}
```

- [ ] **Step 4: Create container handler module**

```rust
// crates/crux-agentic/src/container.rs
use crux_script::HandlerRegistry;
use serde_json::{json, Value};

use crate::adapters::container_client::MockContainerClient;
use crate::adapters::container_client::ContainerClient;

/// Register container step handlers.
pub fn register(registry: &mut HandlerRegistry) {
    registry.register("container::run", |input: Value| {
        Box::pin(async move { handle_run(input).await })
    });
    registry.register("container::wait", |input: Value| {
        Box::pin(async move { handle_wait(input).await })
    });
}

async fn handle_run(input: Value) -> Result<Value, String> {
    let image = input["image"].as_str().unwrap_or("alpine:latest");
    let cmd: Vec<String> = input["cmd"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let memory_mb = input["memory_mb"].as_u64().unwrap_or(512);
    let cpu_millicores = input["cpu_millicores"].as_u64().unwrap_or(1000);
    let timeout = input["timeout_seconds"].as_u64().unwrap_or(300);

    let client = MockContainerClient;
    let handle = client.run(image, &cmd, memory_mb, cpu_millicores, timeout).await?;
    serde_json::to_value(&handle).map_err(|e| e.to_string())
}

async fn handle_wait(input: Value) -> Result<Value, String> {
    let container_id = input["container_id"]
        .as_str()
        .ok_or("missing container_id")?;
    let timeout = input["timeout_seconds"].as_u64().unwrap_or(300);

    let client = MockContainerClient;
    let state = client.wait(container_id, timeout).await?;
    serde_json::to_value(&state).map_err(|e| e.to_string())
}
```

- [ ] **Step 5: Wire modules**

Add to `crates/crux-agentic/src/adapters/mod.rs`:

```rust
pub mod container_client;
```

Add to `crates/crux-agentic/src/lib.rs`:

```rust
pub mod container;
```

Add to `register_all_with_plugins`:

```rust
container::register(registry);
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo nextest run -p crux-agentic container`
Expected: 2 tests pass

- [ ] **Step 7: Commit**

```bash
git add crates/crux-agentic/src/container.rs crates/crux-agentic/src/adapters/container_client.rs crates/crux-agentic/src/adapters/mod.rs crates/crux-agentic/src/lib.rs
git commit -m "feat(agentic): add container::run and container::wait step handlers"
```

---

## Task 7: Harness Handlers in crux-agentic

**Files:**

- Create: `crates/crux-agentic/src/harness.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/crux-agentic/tests/harness.rs
use crux_agentic::harness;
use crux_script::HandlerRegistry;
use serde_json::json;

#[tokio::test]
async fn harness_handlers_registered() {
    let mut registry = HandlerRegistry::new();
    harness::register(&mut registry);
    assert!(registry.get("harness::evolve").is_some());
    assert!(registry.get("harness::canary").is_some());
}

#[tokio::test]
async fn harness_canary_returns_outcome() {
    let mut registry = HandlerRegistry::new();
    harness::register(&mut registry);
    let handler = registry.get("harness::canary").unwrap();
    let input = json!({
        "baseline_profile": {
            "id": "default-v1",
            "resources": {"memory_mb": 512, "cpu_millicores": 1000, "timeout_seconds": 300},
            "network_access": false,
            "allowed_syscalls": ["read", "write"]
        },
        "candidate_profile": {
            "id": "evolved-v2",
            "resources": {"memory_mb": 768, "cpu_millicores": 1000, "timeout_seconds": 300},
            "network_access": false,
            "allowed_syscalls": ["read", "write"]
        },
        "eval_image": "test-suite:latest",
        "eval_cmd": ["./run-benchmarks"]
    });
    let result = handler.call(input).await.unwrap();
    // Mock always returns Promoted
    assert!(result.get("outcome").is_some());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p crux-agentic harness`
Expected: compilation error

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/crux-agentic/src/harness.rs
use crux_runtime::types::evolution::EvolutionOutcome;
use crux_runtime::types::harness::HarnessProfile;
use crux_script::HandlerRegistry;
use serde_json::Value;

/// Register harness step handlers.
pub fn register(registry: &mut HandlerRegistry) {
    registry.register("harness::evolve", |input: Value| {
        Box::pin(async move { handle_evolve(input).await })
    });
    registry.register("harness::canary", |input: Value| {
        Box::pin(async move { handle_canary(input).await })
    });
}

async fn handle_evolve(input: Value) -> Result<Value, String> {
    // In a real implementation, this calls the LLM to propose a diff.
    // For now, return a mock diff proposal.
    let base: HarnessProfile =
        serde_json::from_value(input["base_profile"].clone()).map_err(|e| e.to_string())?;
    let diff = crux_runtime::types::harness::HarnessDiff {
        memory_delta_mb: Some(256),
        ..Default::default()
    };
    let proposed = diff.apply(&base);
    serde_json::to_value(&serde_json::json!({
        "proposed_profile": proposed,
        "diff": diff,
    }))
    .map_err(|e| e.to_string())
}

async fn handle_canary(input: Value) -> Result<Value, String> {
    // In a real implementation, this runs both profiles and compares metrics.
    // Mock: candidate always wins by 15%.
    let candidate: HarnessProfile =
        serde_json::from_value(input["candidate_profile"].clone()).map_err(|e| e.to_string())?;
    let outcome = EvolutionOutcome::Promoted {
        profile_id: candidate.id,
        improvement_pct: 15.0,
    };
    serde_json::to_value(&outcome).map_err(|e| e.to_string())
}
```

- [ ] **Step 4: Wire module**

Add to `crates/crux-agentic/src/lib.rs`:

```rust
pub mod harness;
```

Add to `register_all_with_plugins`:

```rust
harness::register(registry);
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo nextest run -p crux-agentic harness`
Expected: 2 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/crux-agentic/src/harness.rs crates/crux-agentic/src/lib.rs
git commit -m "feat(agentic): add harness::evolve and harness::canary step handlers"
```

---

## Task 8: Deterministic Planner — Metrics and Evolution

**Files:**

- Create: `crates/crux-planner/src/metrics.rs`
- Create: `crates/crux-planner/src/evolution.rs`
- Modify: `crates/crux-planner/src/lib.rs`
- Modify: `crates/crux-planner/Cargo.toml`

- [ ] **Step 1: Write the failing test**

```rust
// crates/crux-planner/src/evolution.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::RunMetrics;
    use crux_runtime::types::harness::{HarnessDiff, HarnessProfile, ResourceHints};

    fn base_profile() -> HarnessProfile {
        HarnessProfile {
            id: "default-v1".into(),
            resources: ResourceHints {
                memory_mb: 512,
                cpu_millicores: 1000,
                timeout_seconds: 300,
            },
            network_access: false,
            allowed_syscalls: vec!["read".into(), "write".into()],
        }
    }

    #[test]
    fn propose_memory_bump_on_oom() {
        let metrics = vec![
            RunMetrics {
                duration_ms: 1200,
                peak_memory_mb: 500,
                exit_code: 137, // OOM killed
                success: false,
            },
            RunMetrics {
                duration_ms: 1100,
                peak_memory_mb: 510,
                exit_code: 137,
                success: false,
            },
        ];
        let planner = EvolutionPlanner::default();
        let diff = planner.propose(&base_profile(), &metrics);
        assert!(diff.memory_delta_mb.is_some());
        assert!(diff.memory_delta_mb.unwrap() > 0);
    }

    #[test]
    fn propose_no_change_when_healthy() {
        let metrics = vec![
            RunMetrics {
                duration_ms: 800,
                peak_memory_mb: 200,
                exit_code: 0,
                success: true,
            },
        ];
        let planner = EvolutionPlanner::default();
        let diff = planner.propose(&base_profile(), &metrics);
        assert!(!diff.has_changes());
    }

    #[test]
    fn propose_timeout_bump_on_slow_runs() {
        let metrics = vec![
            RunMetrics {
                duration_ms: 290_000, // near 300s timeout
                peak_memory_mb: 200,
                exit_code: 0,
                success: true,
            },
            RunMetrics {
                duration_ms: 295_000,
                peak_memory_mb: 200,
                exit_code: 0,
                success: true,
            },
        ];
        let planner = EvolutionPlanner::default();
        let diff = planner.propose(&base_profile(), &metrics);
        assert!(diff.timeout_delta_seconds.is_some());
        assert!(diff.timeout_delta_seconds.unwrap() > 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p crux-planner evolution`
Expected: compilation error

- [ ] **Step 3: Write metrics types**

```rust
// crates/crux-planner/src/metrics.rs
use serde::{Deserialize, Serialize};

/// Metrics from a single container run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetrics {
    pub duration_ms: u64,
    pub peak_memory_mb: u64,
    pub exit_code: i32,
    pub success: bool,
}
```

- [ ] **Step 4: Write evolution planner**

```rust
// crates/crux-planner/src/evolution.rs
use crux_runtime::types::harness::{HarnessDiff, HarnessProfile};

use crate::metrics::RunMetrics;

/// Deterministic planner: given metrics history, propose a profile diff.
#[derive(Debug, Clone)]
pub struct EvolutionPlanner {
    /// If peak memory exceeds this fraction of limit, propose a bump.
    pub memory_pressure_threshold: f64,
    /// If duration exceeds this fraction of timeout, propose a bump.
    pub timeout_pressure_threshold: f64,
    /// How much to bump memory by (as fraction of current).
    pub memory_bump_factor: f64,
    /// How much to bump timeout by (as fraction of current).
    pub timeout_bump_factor: f64,
}

impl Default for EvolutionPlanner {
    fn default() -> Self {
        Self {
            memory_pressure_threshold: 0.9,
            timeout_pressure_threshold: 0.9,
            memory_bump_factor: 0.5,
            timeout_bump_factor: 0.5,
        }
    }
}

impl EvolutionPlanner {
    /// Analyze metrics and produce a diff. Returns empty diff if no changes needed.
    pub fn propose(&self, profile: &HarnessProfile, metrics: &[RunMetrics]) -> HarnessDiff {
        if metrics.is_empty() {
            return HarnessDiff::default();
        }

        let mut diff = HarnessDiff::default();

        // Check for OOM kills (exit code 137)
        let oom_count = metrics.iter().filter(|m| m.exit_code == 137).count();
        if oom_count > 0 {
            let bump = (profile.resources.memory_mb as f64 * self.memory_bump_factor) as i64;
            diff.memory_delta_mb = Some(bump.max(128));
        }

        // Check memory pressure (non-OOM but close to limit)
        if diff.memory_delta_mb.is_none() {
            let max_peak = metrics.iter().map(|m| m.peak_memory_mb).max().unwrap_or(0);
            let pressure = max_peak as f64 / profile.resources.memory_mb as f64;
            if pressure > self.memory_pressure_threshold {
                let bump = (profile.resources.memory_mb as f64 * self.memory_bump_factor) as i64;
                diff.memory_delta_mb = Some(bump.max(64));
            }
        }

        // Check timeout pressure
        let timeout_ms = profile.resources.timeout_seconds * 1000;
        let max_duration = metrics.iter().map(|m| m.duration_ms).max().unwrap_or(0);
        let time_pressure = max_duration as f64 / timeout_ms as f64;
        if time_pressure > self.timeout_pressure_threshold {
            let bump =
                (profile.resources.timeout_seconds as f64 * self.timeout_bump_factor) as i64;
            diff.timeout_delta_seconds = Some(bump.max(30));
        }

        diff
    }
}
```

- [ ] **Step 5: Wire modules and update Cargo.toml**

Add to `crates/crux-planner/Cargo.toml` under `[dependencies]`:

```toml
crux-runtime = { path = "../crux-runtime", version = "0.2.5" }
serde = { workspace = true }
serde_json = { workspace = true }
```

Update `crates/crux-planner/src/lib.rs`:

```rust
//! crux-planner — goal-to-pipeline generation for crux-script.
//!
//! Two paths:
//! - Path A (LLM): lives in `crux-agentic::planner`
//! - Path B (deterministic): `EvolutionPlanner` for metrics-driven profile changes

use serde::{Deserialize, Serialize};

pub mod evolution;
pub mod metrics;

/// A user-facing goal to be translated into a pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub description: String,
    pub constraints: Vec<String>,
}

/// Parsed intent extracted from a goal — intermediate representation
/// between natural language and a concrete pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub goal: String,
    pub input_source: Option<String>,
    pub output_destination: Option<String>,
    pub constraints: serde_json::Value,
    pub preferences: serde_json::Value,
}

/// Configuration for deterministic pipeline generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerConfig {
    pub max_steps: usize,
    pub allowed_handlers: Vec<String>,
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo nextest run -p crux-planner evolution`
Expected: 3 tests pass

- [ ] **Step 7: Commit**

```bash
git add crates/crux-planner/
git commit -m "feat(planner): add EvolutionPlanner with metrics-driven profile diffs"
```

---

## Task 9: Proc Macro — #[crux::harness]

**Files:**

- Create: `crates/crux-macros/src/harness.rs`
- Modify: `crates/crux-macros/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add integration test in `crates/crux/tests/harness_macro.rs`:

```rust
// crates/crux/tests/harness_macro.rs
use crux::prelude::*;

#[crux::harness]
pub struct MyHarness {
    pub memory_mb: u64,
    pub cpu_millicores: u64,
    pub timeout_seconds: u64,
    pub network_access: bool,
}

#[test]
fn harness_generates_to_profile() {
    let h = MyHarness {
        memory_mb: 1024,
        cpu_millicores: 2000,
        timeout_seconds: 600,
        network_access: true,
    };
    let profile = h.to_profile("my-harness-v1");
    assert_eq!(profile.id, "my-harness-v1");
    assert_eq!(profile.resources.memory_mb, 1024);
    assert!(profile.network_access);
}

#[test]
fn harness_generates_default() {
    let h = MyHarness::default();
    assert_eq!(h.memory_mb, 512);
    assert_eq!(h.timeout_seconds, 300);
    assert!(!h.network_access);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p crux harness_macro`
Expected: compilation error — no `harness` attribute

- [ ] **Step 3: Write the macro expansion**

```rust
// crates/crux-macros/src/harness.rs
use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse2};

pub fn expand(_attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = parse2(item)?;
    let name = &input.ident;
    let vis = &input.vis;

    // Extract fields (only works on named structs)
    let fields = match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            syn::Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "#[crux::harness] requires a struct with named fields",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "#[crux::harness] can only be applied to structs",
            ))
        }
    };

    let field_defs = fields.iter();

    Ok(quote! {
        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
        #vis struct #name {
            #(#field_defs,)*
        }

        impl Default for #name {
            fn default() -> Self {
                Self {
                    memory_mb: 512,
                    cpu_millicores: 1000,
                    timeout_seconds: 300,
                    network_access: false,
                }
            }
        }

        impl #name {
            /// Convert this harness config into a `HarnessProfile`.
            pub fn to_profile(&self, id: &str) -> ::crux_runtime::types::harness::HarnessProfile {
                ::crux_runtime::types::harness::HarnessProfile {
                    id: id.to_string(),
                    resources: ::crux_runtime::types::harness::ResourceHints {
                        memory_mb: self.memory_mb,
                        cpu_millicores: self.cpu_millicores,
                        timeout_seconds: self.timeout_seconds,
                    },
                    network_access: self.network_access,
                    allowed_syscalls: Vec::new(),
                }
            }
        }
    })
}
```

- [ ] **Step 4: Register the macro**

Add to `crates/crux-macros/src/lib.rs`:

```rust
mod harness;

/// Marks a struct as a harness profile configuration.
///
/// Generates `Default`, `Serialize`/`Deserialize`, and a `to_profile()` method.
#[proc_macro_attribute]
pub fn harness(attr: TokenStream, item: TokenStream) -> TokenStream {
    harness::expand(attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo nextest run -p crux harness_macro`
Expected: 2 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/crux-macros/src/harness.rs crates/crux-macros/src/lib.rs crates/crux/tests/harness_macro.rs
git commit -m "feat(macros): add #[crux::harness] proc macro"
```

---

## Task 10: Proc Macro — #[crux::evolve]

**Files:**

- Create: `crates/crux-macros/src/evolve.rs`
- Modify: `crates/crux-macros/src/lib.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/crux/tests/evolve_macro.rs
use crux::prelude::*;

/// An evolving agent wraps an inner agent with speculate(candidate vs baseline).
#[crux::evolve]
async fn optimize_container(profile: HarnessProfile) -> Crux<HarnessProfile> {
    // The macro wraps this body in speculation logic:
    // 1. Call inner body to get candidate profile
    // 2. Speculate candidate vs baseline
    // 3. Return winner
    let mut candidate = profile.clone();
    candidate.resources.memory_mb += 256;
    candidate.id = format!("{}-evolved", profile.id);
    Ok(candidate)
}

#[tokio::test]
async fn evolve_macro_produces_agent_struct() {
    // The macro should generate OptimizeContainerAgent
    let _name = OptimizeContainerAgent::name();
    assert_eq!(_name, "optimize_container");
}

#[tokio::test]
async fn evolve_macro_runs_function() {
    let profile = HarnessProfile {
        id: "test-v1".into(),
        resources: ResourceHints {
            memory_mb: 512,
            cpu_millicores: 1000,
            timeout_seconds: 300,
        },
        network_access: false,
        allowed_syscalls: vec![],
    };
    let result = optimize_container(profile).await;
    assert!(result.value().is_ok());
    let output = result.value().as_ref().unwrap();
    assert_eq!(output.resources.memory_mb, 768);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p crux evolve_macro`
Expected: compilation error — no `evolve` attribute

- [ ] **Step 3: Write the macro expansion**

```rust
// crates/crux-macros/src/evolve.rs
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{ItemFn, parse2};

use crate::agent::{extract_crux_inner_type, to_pascal_case};

/// `#[crux::evolve]` is syntactic sugar over `#[crux::agent]` that additionally
/// records the execution as an evolution step (StepKind::Speculation with
/// "evolve" prefix). For now it generates the same output as `#[crux::agent]`
/// with the agent struct suffixed as `Agent`.
pub fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    // Reuse the agent expansion — evolve is agent + semantic marker
    let func: ItemFn = parse2(item.clone())?;
    let fn_name = &func.sig.ident;
    let agent_struct = format_ident!("{}Agent", to_pascal_case(&fn_name.to_string()));

    // Delegate to agent::expand for the core generation
    let base = crate::agent::expand(attr, item)?;

    // Add an evolution marker impl
    let extended = quote! {
        #base

        impl #agent_struct {
            /// Marker: this agent was generated with `#[crux::evolve]`.
            pub fn is_evolution_agent() -> bool {
                true
            }
        }
    };

    Ok(extended)
}
```

Make `extract_crux_inner_type` and `to_pascal_case` `pub(crate)` in
`crates/crux-macros/src/agent.rs`.

- [ ] **Step 4: Register the macro**

Add to `crates/crux-macros/src/lib.rs`:

```rust
mod evolve;

/// Marks an async function as an evolution agent.
///
/// Same as `#[crux::agent]` but semantically marks the function as part of
/// the harness evolution loop. Generates an `is_evolution_agent()` method.
#[proc_macro_attribute]
pub fn evolve(attr: TokenStream, item: TokenStream) -> TokenStream {
    evolve::expand(attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo nextest run -p crux evolve_macro`
Expected: 2 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/crux-macros/src/evolve.rs crates/crux-macros/src/lib.rs crates/crux-macros/src/agent.rs crates/crux/tests/evolve_macro.rs
git commit -m "feat(macros): add #[crux::evolve] proc macro for evolution agents"
```

---

## Task 11: Terminal Approval Gate Adapter

**Files:**

- Create: `crates/crux-agentic/src/adapters/terminal_approval.rs`
- Modify: `crates/crux-agentic/src/adapters/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/crux-agentic/tests/terminal_approval.rs
use crux_agentic::adapters::terminal_approval::AutoApproveGate;
use crux_runtime::approval::{ApprovalDecision, ApprovalGate, ApprovalRequest, RiskLevel};

#[tokio::test]
async fn auto_approve_gate_approves_low_risk() {
    let gate = AutoApproveGate::new(RiskLevel::Medium);
    let req = ApprovalRequest {
        summary: "bump memory".into(),
        diff_description: "memory_mb: 512 -> 768".into(),
        risk_level: RiskLevel::Low,
    };
    let decision = gate.request_approval(&req).await;
    assert!(matches!(decision, ApprovalDecision::Approved));
}

#[tokio::test]
async fn auto_approve_gate_denies_above_threshold() {
    let gate = AutoApproveGate::new(RiskLevel::Low);
    let req = ApprovalRequest {
        summary: "enable network".into(),
        diff_description: "network: false -> true".into(),
        risk_level: RiskLevel::Medium,
    };
    let decision = gate.request_approval(&req).await;
    assert!(matches!(decision, ApprovalDecision::Denied { .. }));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p crux-agentic terminal_approval`
Expected: compilation error

- [ ] **Step 3: Write minimal implementation**

```rust
// crates/crux-agentic/src/adapters/terminal_approval.rs
use crux_runtime::approval::{ApprovalDecision, ApprovalGate, ApprovalRequest, RiskLevel};

/// Auto-approve gate that approves anything at or below the configured risk threshold.
/// For testing and non-interactive environments.
pub struct AutoApproveGate {
    max_auto_approve: RiskLevel,
}

impl AutoApproveGate {
    pub fn new(max_auto_approve: RiskLevel) -> Self {
        Self { max_auto_approve }
    }
}

impl RiskLevel {
    fn severity(self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::Critical => 4,
        }
    }
}

impl ApprovalGate for AutoApproveGate {
    async fn request_approval(&self, request: &ApprovalRequest) -> ApprovalDecision {
        if request.risk_level.severity() <= self.max_auto_approve.severity() {
            ApprovalDecision::Approved
        } else {
            ApprovalDecision::Denied {
                reason: format!(
                    "risk level {:?} exceeds auto-approve threshold {:?}",
                    request.risk_level, self.max_auto_approve
                ),
            }
        }
    }
}

/// Interactive terminal gate — prints the request and reads y/n from stdin.
/// Not usable in tests; use `AutoApproveGate` for testing.
pub struct TerminalApprovalGate;

impl ApprovalGate for TerminalApprovalGate {
    async fn request_approval(&self, request: &ApprovalRequest) -> ApprovalDecision {
        eprintln!("--- APPROVAL REQUIRED ---");
        eprintln!("Summary: {}", request.summary);
        eprintln!("Risk: {:?}", request.risk_level);
        eprintln!("Diff: {}", request.diff_description);
        eprintln!("Approve? [y/N]: ");

        let answer = tokio::task::spawn_blocking(|| {
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).ok();
            buf.trim().to_lowercase()
        })
        .await
        .unwrap_or_default();

        if answer == "y" || answer == "yes" {
            ApprovalDecision::Approved
        } else {
            ApprovalDecision::Denied {
                reason: "user denied".into(),
            }
        }
    }
}
```

- [ ] **Step 4: Wire module**

Add to `crates/crux-agentic/src/adapters/mod.rs`:

```rust
pub mod terminal_approval;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo nextest run -p crux-agentic terminal_approval`
Expected: 2 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/crux-agentic/src/adapters/terminal_approval.rs crates/crux-agentic/src/adapters/mod.rs
git commit -m "feat(agentic): add AutoApproveGate and TerminalApprovalGate adapters"
```

---

## Task 12: Full Integration — Wire Everything and Verify

**Files:**

- Modify: `crates/crux-runtime/src/lib.rs` (final prelude exports)
- No new files

- [ ] **Step 1: Ensure all prelude exports are present**

Verify `crates/crux-runtime/src/lib.rs` prelude includes:

```rust
pub use crate::approval::{ApprovalDecision, ApprovalGate, ApprovalRequest, RiskLevel};
pub use crate::safety::{SafetyPolicy, SafetyViolation};
pub use crate::types::evolution::EvolutionOutcome;
pub use crate::types::harness::{HarnessDiff, HarnessProfile, ResourceHints};
```

- [ ] **Step 2: Run full workspace check**

Run: `cargo check --workspace`
Expected: clean build, no errors

- [ ] **Step 3: Run full test suite**

Run: `cargo nextest run --workspace`
Expected: all tests pass

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings

- [ ] **Step 5: Commit any final fixups**

```bash
git add -A
git commit -m "chore: wire orchestrator types into prelude, fix clippy"
```

---

## Summary

| Task | Crate        | What                                             |
| ---- | ------------ | ------------------------------------------------ |
| 1    | crux-runtime | `HarnessProfile`, `ResourceHints`, `HarnessDiff` |
| 2    | crux-runtime | `EvolutionOutcome`                               |
| 3    | crux-runtime | `SafetyPolicy` trait                             |
| 4    | crux-runtime | `ApprovalGate` trait                             |
| 5    | crux-runtime | `on_approval_required` hook                      |
| 6    | crux-agentic | `container::run`, `container::wait` handlers     |
| 7    | crux-agentic | `harness::evolve`, `harness::canary` handlers    |
| 8    | crux-planner | `EvolutionPlanner`, `RunMetrics`                 |
| 9    | crux-macros  | `#[crux::harness]`                               |
| 10   | crux-macros  | `#[crux::evolve]`                                |
| 11   | crux-agentic | `AutoApproveGate`, `TerminalApprovalGate`        |
| 12   | all          | Integration verification                         |
