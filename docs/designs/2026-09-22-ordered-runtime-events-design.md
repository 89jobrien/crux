# Design: Ordered Runtime Events

## Goal

Define one typed, monotonically ordered event record shared by runtime emission, subscribers,
durable replay, filtering, analytics, and CLI JSONL export.

## Approved Approach

Use `Emission` as the canonical payload inside an ordered `RuntimeEvent` envelope, while keeping
`StepEvent` conversion and `CruxCtx::set_event_sender` as compatibility entry points.

## Context Map

### Files to Modify

| File | Purpose | Change |
| --- | --- | --- |
| `crates/crux-types/src/emission.rs` | Canonical payload and sink adapters | Add the ordered envelope and filter; make sinks consume it |
| `crates/crux-domain/src/event.rs` | Legacy step event API | Add lossless conversion into `Emission` |
| `crates/crux-domain/src/pipeline.rs` | Subscriber fan-out | Sequence and publish `RuntimeEvent` values |
| `crates/crux-domain/Cargo.toml` | Domain dependencies | Add the existing workspace `crux-types` dependency |
| `crates/crux-runtime/src/ctx.rs` | Runtime recording | Emit canonical `Emission` values through the ordered sender |
| `crates/crux-runtime/src/event_log.rs` | Durable event storage | Persist and filter the same `RuntimeEvent` values |
| `crates/crux-runtime/src/event_sink.rs` | Runtime event tests | Replace fragmented-surface tests with ordered-stream tests |
| `crates/crux-runtime/src/observability.rs` | JSONL export | Serialize `RuntimeEvent` records |
| `crates/crux-runtime/src/lib.rs` | Public runtime facade | Re-export the unified event API |
| `crates/crux-cli/src/bin/crux/main.rs` | CLI arguments | Add an explicit event JSONL path option |
| `crates/crux-cli/src/bin/crux/run.rs` | CLI execution output | Persist canonical runtime event JSONL |
| `crates/crux/tests/substrate_integration.rs` | Cross-crate wiring | Assert runtime/subscriber ordering |
| `crates/crux-runtime/TODO.md` | Runtime backlog | Mark the unified event stream item complete |

### Dependencies

- `crux-domain` may depend on `crux-types`; `crux-types` has no dependency on `crux-domain`, so no
  cycle is introduced.
- `crux-runtime` already depends on both crates and remains the integration boundary.
- `crux-cli` already depends on `crux-runtime` and consumes its JSONL adapter.

### Existing Coverage

- `crux-types/src/emission.rs` covers serialization, JSONL, metrics, and sink fan-out.
- `crux-domain/src/lib.rs` covers pipeline delivery to multiple subscribers.
- `crux-runtime/src/event_sink.rs` covers context emission, durable replay, and trace JSONL.
- `crux/tests/substrate_integration.rs` covers planner/runtime/pipeline composition.

### Risk

- `EventSink::emit` changes from `Emission` to `RuntimeEvent`; this is a public pre-1.0 API change.
- `EventReceiver` changes from `StepEvent` to `RuntimeEvent`; legacy producers remain supported by
  `EventSender::send(StepEvent)`.
- JSONL gains ordering and context fields while retaining the flattened `kind` payload tag.

## Crate Ownership

- **`crux-types`** owns the serializable `RuntimeEvent`, `RuntimeEventFilter`, `Emission`, and the
  sink port because downstream tools need the wire contract without the runtime.
- **`crux-domain`** owns the Tokio sequencing/fan-out pipeline and the legacy `StepEvent` adapter.
- **`crux-runtime`** owns context recording, durable log adaptation, filtering, and trace export.
- **`crux-cli`** only selects the JSONL file adapter; it does not define another event shape.

The explicit issue outcome crosses these four existing layers; no new crate is introduced.

## Public API

### Types

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub sequence: u64,
    pub emitted_at: chrono::DateTime<chrono::Utc>,
    pub trace_id: Option<CruxId>,
    pub agent: Option<String>,
    #[serde(flatten)]
    pub emission: Emission,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeEventFilter {
    pub replay_only: bool,
    pub step_name: Option<String>,
}
```

### Event methods

```rust
impl Emission {
    pub fn step_name(&self) -> Option<&str>;
    pub fn is_replay(&self) -> bool;
}

impl RuntimeEvent {
    pub fn matches(&self, filter: &RuntimeEventFilter) -> bool;
}
```

### Sink port

```rust
pub trait EventSink: Send + Sync {
    fn emit(&self, event: RuntimeEvent);
}
```

### Ordered pipeline

```rust
impl EventPipeline {
    pub fn new(capacity: usize) -> Self;
    pub fn with_sinks(capacity: usize, sinks: Vec<Arc<dyn EventSink>>) -> Self;
    pub fn sender(&self) -> EventSender;
    pub fn subscribe(&self) -> EventReceiver;
}

impl EventSender {
    pub fn send(&self, event: StepEvent) -> Result<usize, EventSendError>;
    pub fn emit(
        &self,
        trace_id: Option<CruxId>,
        agent: Option<&str>,
        emission: Emission,
    ) -> RuntimeEvent;
}
```

`EventSender` serializes sequence assignment, sink delivery, and broadcast under one lock so two
concurrent producers cannot observe sequence numbers out of delivery order.

### Durable log

```rust
impl EventLog {
    pub fn append(&self, event: &RuntimeEvent) -> Result<(), EventLogError>;
    pub fn replay(&self) -> Result<Vec<RuntimeEvent>, EventLogError>;
    pub fn replay_filtered(
        &self,
        filter: &RuntimeEventFilter,
    ) -> Result<Vec<RuntimeEvent>, EventLogError>;
}

impl EventSink for EventLog {
    fn emit(&self, event: RuntimeEvent);
}
```

`LoggedEvent` remains as a deprecated type alias to `RuntimeEvent` where source compatibility is
possible.

### CLI JSONL

```rust
pub struct RunConfig<'a> {
    pub events_jsonl_path: Option<&'a str>,
    // existing fields unchanged
}
```

`crux run --events-jsonl <PATH>` writes one flattened `RuntimeEvent` per line without changing the
existing `--json` result contract.

## Data Flow

1. `CruxCtx` converts lifecycle activity directly into `Emission` payloads.
2. `EventSender` assigns the next sequence and context, producing one `RuntimeEvent`.
3. The sender delivers that exact record to configured `EventSink`s and broadcast subscribers in
   the same critical section.
4. `EventLog`, `JsonlWriter`, and `MetricsSink` consume the same record without reshaping it.
5. Replay and analytics select records through `RuntimeEventFilter` or inspect `Emission` methods.
6. The CLI JSONL adapter writes the same serialized shape while preserving existing CLI result
   output modes.

## Hexagonal Boundaries

- **Port**: `EventSink` in `crux_types::emission`.
- **Adapters**: `JsonlWriter`, `MetricsSink`, `MultiSink`, and `EventLog`.
- **In-process transport**: `EventPipeline` in `crux_domain::pipeline`.

## Compatibility

- `StepEvent` remains serializable with its existing wire shape.
- `EventSender::send(StepEvent)` and `CruxCtx::set_event_sender` remain available.
- Existing `--json` CLI output remains compact result JSON.
- Existing plain `Emission` JSON remains readable as the flattened payload portion, but custom
  `EventSink` implementations must adopt the ordered envelope.

## Out of Scope

- Distributed sequence allocation across processes.
- Guaranteed delivery to lagging broadcast subscribers.
- Event batching or external broker integration.
- Expanding agent request/reply behavior.

## Risk

- [x] Breaking API changes: `EventSink` and subscriber item types become ordered records.
- [ ] New external dependency: none.
- [ ] Feature flag required: none.
