# Crux Conformance Specification

Port/adapter boundary contracts for the crux hexagonal architecture. Each section documents
one trait (port), its method signatures, numbered contract clauses, and the adapters that must
satisfy it.

---

## 1. `RegistryBackend`

**Source:** `crates/crux-runtime/src/registry/backend.rs`

```rust
pub trait RegistryBackend: Send + Sync {
    fn get(&self, id: &TaskId)
        -> impl Future<Output = Result<Option<Vec<u8>>, RegistryErr>> + Send;
    fn put(&self, id: &TaskId, data: Vec<u8>)
        -> impl Future<Output = Result<(), RegistryErr>> + Send;
    fn list(&self, prefix: &str)
        -> impl Future<Output = Result<Vec<TaskId>, RegistryErr>> + Send;
    fn cas(&self, id: &TaskId, expected: Vec<u8>, new: Vec<u8>)
        -> impl Future<Output = Result<bool, RegistryErr>> + Send;
}
```

### Contract Clauses

1. `get` on an absent key returns `Ok(None)`.
2. `get` after `put` with the same `TaskId` returns `Ok(Some(data))` where `data` equals the
   bytes passed to `put`.
3. `put` is idempotent-overwrite: a second `put` with the same key replaces the previous value.
4. `list(prefix)` returns every `TaskId` whose string representation starts with `prefix`;
   it returns an empty vec when no keys match, never `Err`.
5. `cas` returns `Ok(true)` when `expected` matches the stored bytes and atomically replaces them
   with `new`.
6. `cas` returns `Ok(false)` when `expected` does not match the stored bytes; stored value is
   unchanged.
7. `cas` returns `Ok(false)` (not `Err`) when the key is absent.

### Adapters

| Adapter           | Location                                 | Feature flag |
| ----------------- | ---------------------------------------- | ------------ |
| `InMemoryBackend` | `crux-runtime/src/registry/in_memory.rs` | (default)    |
| `RedbBackend`     | `crux-runtime/src/registry/redb.rs`      | `redb`       |

---

## 2. `Context`

**Source:** `crates/crux-runtime/src/context.rs`

```rust
pub trait Context: Send {
    fn step<F, Fut, T>(&mut self, name: &str, f: F)
        -> impl Future<Output = Result<T, CruxErr>> + Send;
    fn step_keyed<F, Fut, T, K>(&mut self, name: &str, key: &K, f: F)
        -> impl Future<Output = Result<T, CruxErr>> + Send;
    fn step_with_confidence<F, Fut, T>(&mut self, name: &str, confidence: f32, f: F)
        -> impl Future<Output = Result<T, CruxErr>> + Send;
    fn step_retryable<F, Fut, T>(&mut self, name: &str, confidence: f32, make_fut: F)
        -> impl Future<Output = Result<T, CruxErr>> + Send;
    fn step_stream<F, S, T>(&mut self, name: &str, f: F)
        -> impl Future<Output = Result<T, CruxErr>> + Send;
    fn budget(&self) -> &Budget;
    fn remaining_budget(&self) -> u64;
    fn consume_budget(&mut self, amount: u64);
    fn set_budget(&mut self, budget: Budget);
    fn set_max_retries(&mut self, n: u32);
    fn step_count(&self) -> u32;
    fn snapshot_steps(&self) -> &[Step];
    fn on_low_confidence<F, Fut>(&mut self, threshold: f32, handler: F);
    fn on_step_failure<F, Fut>(&mut self, handler: F);
    fn on_budget_exceeded<F, Fut>(&mut self, handler: F);
}
```

### Contract Clauses

1. `step` executes the closure and records the result as a `Step` appended to the trace.
2. `step` propagates the closure's `Err` as a `CruxErr` without recording a successful step.
3. `step_with_confidence` records the step with the supplied confidence score (0.0–1.0).
4. `step_keyed` uses the content key hash to distinguish steps with the same name during replay.
5. `step_retryable` retries up to `set_max_retries` times on `Err` before propagating.
6. `step_stream` records each yielded `Ok(T)` as an event; the last value becomes step output.
   The first `Err` fails the step immediately.
7. `budget()` returns the current `Budget`; defaults to `Budget::default()` (unlimited).
8. `consume_budget` decrements available units; `remaining_budget` reflects the delta.
9. `step_count` is the number of steps recorded since context creation.
10. `snapshot_steps` returns a slice of all recorded steps in insertion order.
11. Lifecycle hooks (`on_low_confidence`, `on_step_failure`, `on_budget_exceeded`) are invoked
    when the named condition occurs; they return a `Recovery<Value>` that controls step retry,
    skip, or escalation.

### Adapters

| Adapter                | Location                  |
| ---------------------- | ------------------------- |
| `CruxCtx` (production) | `crux-runtime/src/ctx.rs` |

---

## 3. `Agent`

**Source:** `crates/crux-runtime/src/agent.rs`

```rust
pub trait Agent: Send + Sync + 'static {
    type Input: Serialize + DeserializeOwned + Send;
    type Output: Serialize + DeserializeOwned + Send;

    fn name() -> &'static str;
    fn run(ctx: &mut CruxCtx, input: Self::Input)
        -> impl Future<Output = Result<Self::Output, CruxErr>> + Send;
    fn budget() -> Budget { Budget::default() }
    fn on_low_confidence(_score: f32) -> Recovery<Self::Output> { Recovery::Continue }
    fn on_step_failure(_err: &CruxErr) -> Recovery<Self::Output> { Recovery::Propagate }
}
```

### Contract Clauses

1. `name()` returns a stable, non-empty identifier for the agent. The `#[crux::agent]` macro
   sets this to the decorated function name.
2. `run` must record at least one step via the supplied `CruxCtx`.
3. `run` returns `Ok(output)` on success; the output must be serde-serializable for trace storage.
4. `budget()` defaults to `Budget::default()` (unlimited) unless overridden.
5. `on_low_confidence` defaults to `Recovery::Continue`; implementors may substitute or escalate.
6. `on_step_failure` defaults to `Recovery::Propagate`; implementors may retry or substitute.
7. `Input` and `Output` associated types must implement both `Serialize` and `DeserializeOwned`
   to support replay.

### Adapters

| Adapter                            | Source                                       |
| ---------------------------------- | -------------------------------------------- |
| Macro-generated `FooAgent` structs | `#[crux::agent]` proc macro in `crux-macros` |
| Hand-written `Agent` impls         | any downstream crate                         |

---

## 4. `LlmProvider`

**Source:** `crates/crux-agentic/src/provider.rs`

```rust
pub trait LlmProvider: Send + Sync + 'static {
    fn complete(&self, req: LlmRequest)
        -> impl Future<Output = Result<LlmResponse, CruxErr>> + Send;
}

pub struct LlmRequest { pub prompt: String, pub system: Option<String>, pub max_tokens: u32 }
pub struct LlmResponse { pub text: String, pub provider: String,
                         pub metadata: Option<serde_json::Value> }
```

### Contract Clauses

1. `complete` must return `Ok(LlmResponse)` on success; `LlmResponse.text` must be non-empty.
2. `LlmResponse.provider` must identify the backend in `"vendor/model"` form (e.g.
   `"anthropic/claude-sonnet-4-6"`).
3. `LlmResponse.metadata` may be `None`; when present it is opaque and passes through verbatim.
4. `LlmRequest.max_tokens` defaults to `1024`.
5. Both `LlmRequest` and `LlmResponse` are `Serialize + DeserializeOwned`; the serde roundtrip
   must be lossless (required for step replay).
6. `complete` propagates all errors as `CruxErr` — no provider-specific error types cross the
   boundary.

### Adapters

| Adapter           | Location                           | Notes                              |
| ----------------- | ---------------------------------- | ---------------------------------- |
| `StubLlmProvider` | `crux-agentic/src/handlers/llm.rs` | Always returns canned text         |
| Anthropic adapter | `crux-agentic/src/handlers/llm.rs` | Live; requires `ANTHROPIC_API_KEY` |

---

## 5. `SafetyPolicy`

**Source:** `crates/crux-runtime/src/safety.rs`

```rust
pub trait SafetyPolicy: Send + Sync {
    fn validate(&self, diff: &HarnessDiff, base: &HarnessProfile)
        -> Result<(), SafetyViolation>;
    fn requires_approval(&self, diff: &HarnessDiff) -> bool;
}

pub enum SafetyViolation {
    HardCapExceeded { resource: String, limit: u64, proposed: u64 },
    ForbiddenSyscall { syscall: String },
    Custom { reason: String },
}
```

### Contract Clauses

1. `validate` returns `Ok(())` when the proposed diff, applied to `base`, stays within all
   enforced limits.
2. `validate` returns `Err(SafetyViolation::HardCapExceeded)` when `diff.apply(base).memory_mb`
   exceeds the policy's memory cap; `limit` and `proposed` must reflect the actual values.
3. `validate` returns `Err(SafetyViolation::ForbiddenSyscall)` when `diff.syscall_additions`
   contains a syscall the policy prohibits; `syscall` must name the offending call.
4. Forbidden-syscall checks must run before resource cap checks so that `ForbiddenSyscall`
   is reported rather than silently succeeding on a cap-compliant diff.
5. `requires_approval` returns `true` when `diff.network_access_change == Some(true)`.
6. `requires_approval` returns `true` when `diff.syscall_additions` is non-empty, regardless of
   whether those syscalls are forbidden.
7. `requires_approval` returns `false` for resource-only diffs (memory, cpu, timeout deltas only,
   no network or syscall changes).
8. `validate` and `requires_approval` are independent — a diff that passes `validate` may still
   require approval, and vice versa.

### Adapters

| Adapter                       | Location                                  |
| ----------------------------- | ----------------------------------------- |
| `StrictPolicy` (unit tests)   | `crux-runtime/src/safety.rs` inline tests |
| `BoundedPolicy` (conformance) | `crux/tests/conformance/safety_policy.rs` |

---

## 6. `ApprovalGate`

**Source:** `crates/crux-runtime/src/approval.rs`

```rust
pub trait ApprovalGate: Send + Sync {
    fn request_approval(&self, request: &ApprovalRequest)
        -> impl Future<Output = ApprovalDecision> + Send;
}

pub struct ApprovalRequest {
    pub summary: String,
    pub diff_description: String,
    pub risk_level: RiskLevel,
}

#[serde(rename_all = "snake_case", tag = "decision")]
pub enum ApprovalDecision {
    Approved,
    Denied { reason: String },
    Deferred { timeout_seconds: u64 },
}

pub enum RiskLevel { Low, Medium, High, Critical }
```

### Contract Clauses

1. `request_approval` must return one of `Approved`, `Denied`, or `Deferred` — never panics or
   hangs indefinitely.
2. `Denied.reason` must be a non-empty human-readable string.
3. `Deferred.timeout_seconds` must be greater than zero and represent the wall-clock window in
   which a follow-up decision is expected.
4. `ApprovalRequest` and `ApprovalDecision` are both `Serialize + Deserialize`; roundtrips must
   be lossless.
5. `ApprovalDecision` serializes as a tagged enum with `"decision"` as the tag key, values
   `"approved"`, `"denied"`, `"deferred"`.
6. `RiskLevel` serializes as `snake_case` strings (`"low"`, `"medium"`, `"high"`, `"critical"`).
7. `ApprovalGate` must be `Send + Sync`; implementations may block the async executor only via
   `tokio::task::spawn_blocking` or equivalent — never via bare blocking I/O.

### Adapters

| Adapter                | Location                                  | Purpose                               |
| ---------------------- | ----------------------------------------- | ------------------------------------- |
| `AutoApproveGate`      | `crux-agentic/src/adapters/`              | Approves all requests unconditionally |
| `TerminalApprovalGate` | `crux-agentic/src/adapters/`              | Interactive TTY prompt                |
| `AlwaysApprove` (test) | `crux/tests/conformance/approval_gate.rs` | Conformance only                      |
| `AlwaysDeny` (test)    | `crux/tests/conformance/approval_gate.rs` | Conformance only                      |

---

## 7. `ContainerClient`

**Source:** `crates/crux-agentic/src/adapters/container_client.rs`

```rust
pub trait ContainerClient: Send + Sync {
    fn run(&self, image: &str, cmd: &[String],
           memory_mb: u64, cpu_millicores: u64, timeout_seconds: u64)
        -> impl Future<Output = Result<ContainerHandle, String>> + Send;
    fn wait(&self, container_id: &str, timeout_seconds: u64)
        -> impl Future<Output = Result<ContainerState, String>> + Send;
}

pub struct ContainerHandle { pub container_id: String, pub state: ContainerState }
pub enum ContainerState {
    Running,
    Stopped { exit_code: i32 },
    Failed { error: String },
}
```

### Contract Clauses

1. `run` returns `Ok(ContainerHandle)` where `handle.state == ContainerState::Running` when the
   container starts successfully.
2. `run` returns `Err(String)` with a human-readable message on failure; the error must not
   be empty.
3. `handle.container_id` must be a non-empty, stable identifier usable as the argument to `wait`.
4. `wait` blocks (async) until the container exits or `timeout_seconds` elapses.
5. `wait` returns `Ok(ContainerState::Stopped { exit_code })` on clean exit; `exit_code` is the
   process exit status.
6. `wait` returns `Err(String)` when the timeout elapses before the container exits.
7. `ContainerState` and `ContainerHandle` are `Serialize + Deserialize`; `ContainerState`
   serializes in `snake_case`.
8. `memory_mb` and `cpu_millicores` are advisory limits — adapters must apply them as resource
   constraints but may round to the nearest unit their runtime supports.

### Adapters

| Adapter                 | Location                                        | Feature flag |
| ----------------------- | ----------------------------------------------- | ------------ |
| `MockContainerClient`   | `crux-agentic/src/adapters/container_client.rs` | (default)    |
| `DockerContainerClient` | same file                                       | `docker`     |
