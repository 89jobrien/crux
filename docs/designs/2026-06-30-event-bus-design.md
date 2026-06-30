# Design: Layered Event Bus (EventSink + MessageRouter)

## Goal

Add observability-as-infrastructure and agent-to-agent request-reply
communication to crux by extending the existing StepEvent + EventPipeline
broadcast system with a JSONL trace writer and addressed messaging.

## Approved Approach

Layered bus: `EventSink` (write-only broadcast) and `MessageRouter`
(addressed request-reply, implies `EventSink`). Extends the existing
`StepEvent`/`EventPipeline` rather than replacing it. Emission is a
first-class enum type (`Emission`) that unifies all observable events
and agent messages.

## Crate Ownership

- **`crux-types`** -- owns `Emission` enum, `EventSink` trait,
  `MessageRouter` trait, `JsonlWriter` adapter, `InMemoryRouter`
  adapter, `NullSink`, `MultiSink`. Chosen because it has minimal
  deps (serde, chrono, ulid) and is consumed by external crates
  (minibox) that need event types without pulling the full runtime.

- **`crux-domain`** -- owns `StepEvent` enum. Unchanged. Bridge
  conversion `Emission -> StepEvent` implemented here via `From`.

- **`crux-runtime`** -- owns `CruxCtx` integration, `BroadcastSink`
  adapter (bridges `Emission` to existing `EventPipeline`),
  `request_and_await()` ergonomic wrapper, `TracingSubscriber`
  adapter (behind `tracing` feature flag). Deletes `trace.rs`.

## Public API

### Core type (`crux-types::emission`)

```rust
/// The single event type that flows through every EventSink.
/// Unifies step lifecycle, combinator lifecycle, runtime internals,
/// and agent-to-agent messaging.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Emission {
    // -- Step lifecycle (broadcast) --
    StepStart { name: String },
    StepComplete { name: String, duration_ms: u64 },
    StepError { name: String, error: String },
    StepSkipped { name: String, reason: String },
    StepDenied { name: String, reason: String },
    StepChunk { name: String, payload: Value },

    // -- Combinator lifecycle (broadcast) --
    DelegateStart { name: String, agent: String },
    DelegateComplete { name: String, agent: String, duration_ms: u64 },
    PipeStart { name: String, stage_count: usize },
    PipeComplete { name: String, duration_ms: u64 },
    JoinAllStart { name: String, arm_count: usize },
    JoinAllComplete { name: String, duration_ms: u64 },
    SpeculateStart { name: String, arm_count: usize },
    SpeculateComplete { name: String, duration_ms: u64 },
    RouteMatched { name: String, confidence: f32, label: String },

    // -- Runtime internals (broadcast) --
    ReplayHit { name: String },
    ReplayMiss { name: String },
    HookDispatched { hook: String, step: String },
    Decision { source: String, key: String, value: Value },

    // -- Agent comms (addressed) --
    Message {
        sender: String,
        recipient: String,
        payload: Value,
    },
    Request {
        sender: String,
        recipient: String,
        correlation_id: CruxId,
        payload: Value,
    },
    Reply {
        sender: String,
        recipient: String,
        correlation_id: CruxId,
        payload: Value,
    },
}

impl Emission {
    /// True if this emission targets a specific agent.
    pub fn is_addressed(&self) -> bool;

    /// The recipient agent name, if addressed.
    pub fn recipient(&self) -> Option<&str>;

    /// The correlation ID, if this is a Request or Reply.
    pub fn correlation_id(&self) -> Option<&CruxId>;
}
```

### Traits (`crux-types::emission`)

```rust
/// Write-only broadcast port. Implementations must be non-fatal --
/// a failed write must never abort the calling workflow.
pub trait EventSink: Send + Sync {
    fn emit(&self, emission: Emission);
}

/// Addressed request-reply port. Every send/request also emits to
/// the underlying EventSink for auditability.
pub trait MessageRouter: EventSink {
    fn send(&self, emission: Emission);
    fn request(&self, emission: Emission) -> CruxId;
    fn recv(&self, agent: &str) -> Option<Emission>;
    fn recv_by_correlation(
        &self,
        agent: &str,
        correlation_id: &CruxId,
    ) -> Option<Emission>;
}
```

### Adapters (`crux-types::emission`)

```rust
/// No-op sink. Default when no sink is configured.
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _emission: Emission) {}
}

/// Fan-out to multiple sinks.
pub struct MultiSink {
    sinks: Vec<Arc<dyn EventSink>>,
}

impl MultiSink {
    pub fn new(sinks: Vec<Arc<dyn EventSink>>) -> Self;
}

impl EventSink for MultiSink {
    fn emit(&self, emission: Emission);
}

/// Appends Emission as JSON lines to a file. Non-fatal on I/O error.
pub struct JsonlWriter {
    path: std::path::PathBuf,
}

impl JsonlWriter {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self;
}

impl EventSink for JsonlWriter {
    fn emit(&self, emission: Emission);
}

/// In-memory mailbox router. Wraps an EventSink for broadcast.
/// Uses Mutex<HashMap<String, VecDeque<Emission>>> for mailboxes.
pub struct InMemoryRouter {
    sink: Box<dyn EventSink>,
    mailboxes: std::sync::Mutex<
        std::collections::HashMap<
            String,
            std::collections::VecDeque<Emission>,
        >,
    >,
}

impl InMemoryRouter {
    pub fn new(sink: Box<dyn EventSink>) -> Self;
}

impl EventSink for InMemoryRouter {
    fn emit(&self, emission: Emission);
}

impl MessageRouter for InMemoryRouter {
    fn send(&self, emission: Emission);
    fn request(&self, emission: Emission) -> CruxId;
    fn recv(&self, agent: &str) -> Option<Emission>;
    fn recv_by_correlation(
        &self,
        agent: &str,
        correlation_id: &CruxId,
    ) -> Option<Emission>;
}
```

### Bridge adapter (`crux-runtime`)

```rust
/// Bridges Emission to the existing EventPipeline (tokio broadcast).
/// Converts Emission variants to StepEvent before sending.
pub struct BroadcastSink {
    tx: tokio::sync::broadcast::Sender<StepEvent>,
}

impl BroadcastSink {
    pub fn new(tx: tokio::sync::broadcast::Sender<StepEvent>) -> Self;
}

impl EventSink for BroadcastSink {
    fn emit(&self, emission: Emission);
}
```

### Conversion (`crux-domain::event`)

```rust
/// Lossless conversion from Emission to StepEvent.
/// Agent comms variants map to StepEvent::Custom.
impl From<Emission> for StepEvent { ... }

/// Conversion from StepEvent to Emission.
impl From<StepEvent> for Emission { ... }
```

### TracingSubscriber (`crux-runtime`, behind `tracing` feature)

```rust
/// Forwards Emissions to tracing::info! events.
/// Replaces the deleted trace.rs macros.
pub struct TracingSubscriber;

impl EventSink for TracingSubscriber {
    fn emit(&self, emission: Emission);
}
```

### CruxCtx changes (`crux-runtime::ctx`)

```rust
pub struct CruxCtx {
    // ... existing fields ...
    // CHANGED: Option<EventSender> -> Arc<dyn EventSink>
    event_sink: Arc<dyn EventSink>,
    // NEW: optional router for agent-to-agent comms
    router: Option<Arc<dyn MessageRouter>>,
}

impl CruxCtx {
    /// Set the event sink. Replaces the default NullSink.
    pub fn set_event_sink(&mut self, sink: Arc<dyn EventSink>);

    /// Attach a message router for agent-to-agent communication.
    /// The router is also used as an EventSink (it implies EventSink).
    pub fn set_router(&mut self, router: Arc<dyn MessageRouter>);

    /// Emit an Emission to the configured sink.
    /// Replaces the private emit(StepEvent) method.
    fn emit_event(&self, emission: Emission);

    /// Send a message to another agent (fire-and-forget).
    pub fn send_message(&self, recipient: &str, payload: Value);

    /// Send a request and await the reply. Uses tokio::sync::oneshot
    /// internally -- does not busy-loop.
    pub async fn request_and_await(
        &self,
        recipient: &str,
        payload: Value,
    ) -> Emission;

    /// Check for incoming messages (non-blocking).
    pub fn recv_message(&self) -> Option<Emission>;

    /// Reply to a received request.
    pub fn reply_to(
        &self,
        correlation_id: &CruxId,
        recipient: &str,
        payload: Value,
    );
}
```

## Data Flow

### Observability (broadcast)

1. `CruxCtx` methods (`step`, `delegate`, `pipe`, `join_all`,
   `speculate`, `route_on_confidence`) call `self.emit_event()`
   with the appropriate `Emission` variant.
2. `emit_event()` calls `self.event_sink.emit(emission)`.
3. If using `MultiSink`, fan-out delivers to all registered sinks:
   - `JsonlWriter` appends as a JSON line to the trace file
   - `BroadcastSink` converts to `StepEvent` and sends through
     the existing `EventPipeline` tokio broadcast channel
   - `TracingSubscriber` (optional) forwards to `tracing::info!`

### Agent-to-agent messaging (addressed)

1. Agent A calls `ctx.send_message("agent_b", payload)` or
   `ctx.request_and_await("agent_b", payload)`.
2. `CruxCtx` constructs an `Emission::Message` or `Emission::Request`
   and passes to `self.router.send()` or `self.router.request()`.
3. `InMemoryRouter` inserts into recipient's mailbox (`VecDeque`)
   and also calls `self.sink.emit()` for auditability.
4. Agent B calls `ctx.recv_message()` or the runtime delivers the
   reply via `tokio::sync::oneshot` for `request_and_await`.

### Composition example

```rust
// In composition root / main.rs:
let jsonl = Arc::new(JsonlWriter::new(".ctx/trace.jsonl"));
let broadcast = Arc::new(BroadcastSink::new(pipeline.sender()));
let multi = Arc::new(MultiSink::new(vec![jsonl, broadcast]));
let router = Arc::new(InMemoryRouter::new(Box::new(multi)));

let mut ctx = CruxCtx::new("my_agent");
ctx.set_event_sink(router.clone());
ctx.set_router(router);
```

## Hexagonal Boundaries

| Type | Role | Location |
|------|------|----------|
| `EventSink` | Port (broadcast) | `crux-types::emission` |
| `MessageRouter` | Port (addressed) | `crux-types::emission` |
| `NullSink` | Adapter (no-op) | `crux-types::emission` |
| `MultiSink` | Adapter (fan-out) | `crux-types::emission` |
| `JsonlWriter` | Adapter (file) | `crux-types::emission` |
| `InMemoryRouter` | Adapter (mailbox) | `crux-types::emission` |
| `BroadcastSink` | Adapter (bridge) | `crux-runtime` |
| `TracingSubscriber` | Adapter (tracing) | `crux-runtime` (behind `tracing` flag) |

## Trace Macro Migration

| Macro | Replaced by | Emission variant |
|-------|------------|------------------|
| `trace_step!` | `self.emit_event(Emission::StepStart { .. })` | `StepStart` |
| `trace_delegate!` | `self.emit_event(Emission::DelegateStart { .. })` | `DelegateStart` |
| `trace_speculate!` | `self.emit_event(Emission::SpeculateStart { .. })` | `SpeculateStart` |
| `trace_join_all!` | `self.emit_event(Emission::JoinAllStart { .. })` | `JoinAllStart` |
| `trace_pipe!` | `self.emit_event(Emission::PipeStart { .. })` | `PipeStart` |
| `trace_replay_hit!` | `self.emit_event(Emission::ReplayHit { .. })` | `ReplayHit` |
| `trace_replay_miss!` | `self.emit_event(Emission::ReplayMiss { .. })` | `ReplayMiss` |
| `trace_hook!` | `self.emit_event(Emission::HookDispatched { .. })` | `HookDispatched` |
| `trace_route!` | `self.emit_event(Emission::RouteMatched { .. })` | `RouteMatched` |

After migration, delete `crates/crux-runtime/src/trace.rs` and
remove `#[macro_use] mod trace;` from `lib.rs`.

## Test Plan

| Component | Dimension | Location |
|-----------|-----------|----------|
| `Emission` serde round-trip | Unit | `crux-types::emission` inline |
| `Emission` helper methods | Unit | `crux-types::emission` inline |
| `MessageRouter` contract | Conformance | `crux-types::emission` inline |
| `InMemoryRouter` mailbox ops | Unit | `crux-types::emission` inline |
| `InMemoryRouter` FIFO + routing | Property | `crux-types::emission` inline |
| `JsonlWriter` output validity | Unit | `crux-types::emission` inline |
| `NullSink` is no-op | Unit | `crux-types::emission` inline |
| `MultiSink` fans out | Unit | `crux-types::emission` inline |
| `BroadcastSink` converts correctly | Unit | `crux-runtime` inline |
| `Emission <-> StepEvent` round-trip | Unit | `crux-domain::event` inline |
| `request_and_await` two-agent | Integration | `crux-runtime::event_sink` |
| `emit_event` replaces trace macros | Integration | `crux-runtime::event_sink` |
| Trace macro removal (no regression) | Regression | existing test suite passes |

### Conformance suite for `MessageRouter`

```rust
fn assert_router_contract(router: &dyn MessageRouter) {
    // 1. send to agent, recv returns it
    // 2. recv from empty mailbox returns None
    // 3. request returns correlation ID
    // 4. reply with correlation ID is receivable via recv_by_correlation
    // 5. recv_by_correlation ignores non-matching messages
    // 6. FIFO ordering preserved
    // 7. every send/request/reply also emits to EventSink
}
```

### Property test for `InMemoryRouter`

- Strategy: arbitrary interleaving of send/recv across N agents
- Invariants: every message received exactly once, correct agent,
  FIFO order, no cross-mailbox leakage

## Out of Scope

- OTLP / OpenTelemetry export (future adapter)
- Persistent cross-session messaging (session-scoped only)
- Durable message queues / guaranteed delivery
- Replacing `EventPipeline` / `EventSender` in `crux-domain`
- Changes to `StepRecorder`

## Risk

- [ ] Breaking API changes: **No** -- all additions are additive.
  `CruxCtx::event_sender` field changes from `Option<EventSender>`
  to `Arc<dyn EventSink>`, but the field is private. Public method
  `set_event_sender()` is replaced by `set_event_sink()` -- one
  call site in `event_sink.rs` tests and one in
  `substrate_integration.rs` need updating.
- [ ] New external dependency: **No** -- `crux-types` uses only std
- [ ] Feature flag required: **No** new flags -- `tracing` flag reused
- [x] `JsonlWriter` uses `std::fs` (blocking I/O) -- acceptable for
  append-only single-line writes. If buffering is needed later,
  migrate to `crux-runtime` with async I/O.
