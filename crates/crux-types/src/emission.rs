//! Emission — the unified event type for observability and agent messaging.
//!
//! `RuntimeEvent` is the single ordered record that flows through every
//! `EventSink`; `Emission` is its typed payload.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::id::CruxId;

/// The typed payload carried by every ordered runtime event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Emission {
    // -- Step lifecycle (broadcast) --
    StepStart {
        name: String,
    },
    StepComplete {
        name: String,
        duration_ms: u64,
    },
    StepError {
        name: String,
        error: String,
    },
    StepSkipped {
        name: String,
        reason: String,
    },
    StepDenied {
        name: String,
        reason: String,
    },
    StepChunk {
        name: String,
        payload: serde_json::Value,
    },
    /// A completed trace record exported after execution.
    StepRecorded {
        name: String,
        stable_id: Option<String>,
        step_kind: crate::step::StepKind,
        status: crate::step::StepStatus,
        origin: crate::step::StepOrigin,
        confidence: f32,
        started_at: chrono::DateTime<chrono::Utc>,
        duration_ms: u64,
        metadata: HashMap<String, serde_json::Value>,
    },

    // -- Combinator lifecycle (broadcast) --
    DelegateStart {
        name: String,
        agent: String,
    },
    DelegateComplete {
        name: String,
        agent: String,
        duration_ms: u64,
    },
    PipeStart {
        name: String,
        stage_count: usize,
    },
    PipeComplete {
        name: String,
        duration_ms: u64,
    },
    JoinAllStart {
        name: String,
        arm_count: usize,
    },
    JoinAllComplete {
        name: String,
        duration_ms: u64,
    },
    SpeculateStart {
        name: String,
        arm_count: usize,
    },
    SpeculateComplete {
        name: String,
        duration_ms: u64,
    },
    RouteMatched {
        name: String,
        confidence: f32,
        label: String,
    },

    // -- Runtime internals (broadcast) --
    ReplayHit {
        name: String,
    },
    ReplayMiss {
        name: String,
    },
    HookDispatched {
        hook: String,
        step: String,
    },
    Decision {
        source: String,
        key: String,
        value: serde_json::Value,
    },

    // -- Agent comms (addressed) --
    Message {
        sender: String,
        recipient: String,
        payload: serde_json::Value,
    },
    Request {
        sender: String,
        recipient: String,
        correlation_id: CruxId,
        payload: serde_json::Value,
    },
    Reply {
        sender: String,
        recipient: String,
        correlation_id: CruxId,
        payload: serde_json::Value,
    },
}

impl Emission {
    /// Returns the associated step or operation name, when present.
    pub fn step_name(&self) -> Option<&str> {
        match self {
            Self::StepStart { name }
            | Self::StepComplete { name, .. }
            | Self::StepError { name, .. }
            | Self::StepSkipped { name, .. }
            | Self::StepDenied { name, .. }
            | Self::StepChunk { name, .. }
            | Self::StepRecorded { name, .. }
            | Self::DelegateStart { name, .. }
            | Self::DelegateComplete { name, .. }
            | Self::PipeStart { name, .. }
            | Self::PipeComplete { name, .. }
            | Self::JoinAllStart { name, .. }
            | Self::JoinAllComplete { name, .. }
            | Self::SpeculateStart { name, .. }
            | Self::SpeculateComplete { name, .. }
            | Self::RouteMatched { name, .. }
            | Self::ReplayHit { name }
            | Self::ReplayMiss { name } => Some(name),
            Self::HookDispatched { step, .. } => Some(step),
            Self::Decision { .. }
            | Self::Message { .. }
            | Self::Request { .. }
            | Self::Reply { .. } => None,
        }
    }

    /// Returns true for replay hit and miss diagnostics.
    pub fn is_replay(&self) -> bool {
        matches!(self, Self::ReplayHit { .. } | Self::ReplayMiss { .. })
    }

    /// True if this emission targets a specific agent.
    pub fn is_addressed(&self) -> bool {
        matches!(
            self,
            Emission::Message { .. } | Emission::Request { .. } | Emission::Reply { .. }
        )
    }

    /// The recipient agent name, if addressed.
    pub fn recipient(&self) -> Option<&str> {
        match self {
            Emission::Message { recipient, .. }
            | Emission::Request { recipient, .. }
            | Emission::Reply { recipient, .. } => Some(recipient),
            _ => None,
        }
    }

    /// The correlation ID, if this is a Request or Reply.
    pub fn correlation_id(&self) -> Option<&CruxId> {
        match self {
            Emission::Request { correlation_id, .. } | Emission::Reply { correlation_id, .. } => {
                Some(correlation_id)
            }
            _ => None,
        }
    }
}

/// One typed event in the globally ordered runtime stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEvent {
    /// Monotonic sequence assigned by the event pipeline.
    pub sequence: u64,
    /// Time at which the event entered the ordered stream.
    pub emitted_at: chrono::DateTime<chrono::Utc>,
    /// Trace associated with the event, if emitted during a trace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<CruxId>,
    /// Agent associated with the event, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Typed event payload, flattened to retain the existing `kind` JSON tag.
    #[serde(flatten)]
    pub emission: Emission,
}

/// Filter applied to ordered runtime events during replay and analysis.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeEventFilter {
    /// Include only replay hit and miss diagnostics.
    pub replay_only: bool,
    /// Include only events associated with this step or operation name.
    pub step_name: Option<String>,
}

impl RuntimeEvent {
    /// Returns true when this event satisfies every configured filter.
    pub fn matches(&self, filter: &RuntimeEventFilter) -> bool {
        (!filter.replay_only || self.emission.is_replay())
            && filter
                .step_name
                .as_deref()
                .is_none_or(|name| self.emission.step_name() == Some(name))
    }
}

// EventSink trait + adapters

/// Write-only broadcast port. Implementations must be non-fatal —
/// a failed write must never abort the calling workflow.
pub trait EventSink: Send + Sync {
    /// Broadcasts an event without propagating sink failures to the workflow.
    fn emit(&self, event: RuntimeEvent);
}

impl<T: EventSink> EventSink for Arc<T> {
    fn emit(&self, event: RuntimeEvent) {
        (**self).emit(event);
    }
}

/// No-op sink. Default when no sink is configured.
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: RuntimeEvent) {}
}

/// Fan-out to multiple sinks.
pub struct MultiSink {
    sinks: Vec<Arc<dyn EventSink>>,
}

impl MultiSink {
    /// Creates a sink that fans each event out to all supplied sinks.
    pub fn new(sinks: Vec<Arc<dyn EventSink>>) -> Self {
        Self { sinks }
    }
}

impl EventSink for MultiSink {
    fn emit(&self, event: RuntimeEvent) {
        for sink in &self.sinks {
            sink.emit(event.clone());
        }
    }
}

/// Appends ordered runtime events as JSON lines. Non-fatal on I/O error.
pub struct JsonlWriter {
    path: std::path::PathBuf,
}

impl JsonlWriter {
    /// Creates a sink that appends serialized events to the given path.
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl EventSink for JsonlWriter {
    fn emit(&self, event: RuntimeEvent) {
        use std::io::Write;
        let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            return;
        };
        let Ok(json) = serde_json::to_string(&event) else {
            return;
        };
        let _ = writeln!(file, "{json}");
    }
}

/// In-process counters rendered in the Prometheus text exposition format.
#[derive(Default)]
pub struct MetricsSink {
    emissions: std::sync::atomic::AtomicU64,
    completed: std::sync::atomic::AtomicU64,
    failed: std::sync::atomic::AtomicU64,
    duration_ms: std::sync::atomic::AtomicU64,
}

impl MetricsSink {
    /// Render a scrape-ready snapshot of the current counters.
    pub fn render_prometheus(&self) -> String {
        use std::sync::atomic::Ordering;
        format!(
            "crux_emissions_total {}\ncrux_steps_completed_total {}\n\
             crux_steps_failed_total {}\ncrux_step_duration_milliseconds_total {}\n",
            self.emissions.load(Ordering::Relaxed),
            self.completed.load(Ordering::Relaxed),
            self.failed.load(Ordering::Relaxed),
            self.duration_ms.load(Ordering::Relaxed),
        )
    }
}

impl EventSink for MetricsSink {
    fn emit(&self, event: RuntimeEvent) {
        use std::sync::atomic::Ordering;
        self.emissions.fetch_add(1, Ordering::Relaxed);
        match event.emission {
            Emission::StepComplete { duration_ms, .. } => {
                self.completed.fetch_add(1, Ordering::Relaxed);
                self.duration_ms.fetch_add(duration_ms, Ordering::Relaxed);
            }
            Emission::StepError { .. } => {
                self.failed.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// VectorFileSink — JSONL with Vector-friendly envelope (timestamp + source)
// ---------------------------------------------------------------------------

/// Writes ordered runtime events as JSONL with a Vector-friendly envelope.
/// Each line contains `{ "timestamp": ..., "source": "crux", "event": { ... } }`.
/// Configure Vector's `file` source to tail this path.
#[cfg(feature = "vector-file")]
pub struct VectorFileSink {
    path: std::path::PathBuf,
    source_label: String,
}

#[cfg(feature = "vector-file")]
impl VectorFileSink {
    /// Creates a Vector file sink with a custom source label.
    pub fn new(path: impl Into<std::path::PathBuf>, source_label: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            source_label: source_label.into(),
        }
    }

    fn envelope(&self, runtime_event: &RuntimeEvent) -> Option<String> {
        let event = serde_json::to_value(runtime_event).ok()?;
        let wrapper = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "source": self.source_label,
            "event": event,
        });
        serde_json::to_string(&wrapper).ok()
    }
}

#[cfg(feature = "vector-file")]
impl EventSink for VectorFileSink {
    fn emit(&self, event: RuntimeEvent) {
        use std::io::Write;
        let Some(line) = self.envelope(&event) else {
            return;
        };
        let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            return;
        };
        let _ = writeln!(file, "{line}");
    }
}

// VectorHttpSink — POST JSON to Vector's HTTP source

/// Sends ordered runtime events as JSON to Vector's `http` source endpoint.
/// Non-blocking: spawns a tokio task per emit. Failures are silently dropped
/// (EventSink contract: never abort the calling workflow).
#[cfg(feature = "vector-http")]
pub struct VectorHttpSink {
    url: String,
    source_label: String,
    client: reqwest::Client,
}

#[cfg(feature = "vector-http")]
impl VectorHttpSink {
    /// Creates a Vector HTTP sink for the endpoint and source label.
    pub fn new(url: impl Into<String>, source_label: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            source_label: source_label.into(),
            client: reqwest::Client::new(),
        }
    }

    fn envelope(&self, runtime_event: &RuntimeEvent) -> Option<serde_json::Value> {
        let event = serde_json::to_value(runtime_event).ok()?;
        Some(serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "source": self.source_label,
            "event": event,
        }))
    }
}

#[cfg(feature = "vector-http")]
impl EventSink for VectorHttpSink {
    fn emit(&self, event: RuntimeEvent) {
        let Some(body) = self.envelope(&event) else {
            return;
        };
        let client = self.client.clone();
        let url = self.url.clone();
        tokio::spawn(async move {
            let _ = client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;
        });
    }
}

// MessageRouter trait + InMemoryRouter

/// Addressed request-reply port. Every send/request also emits
/// to the underlying EventSink for auditability.
pub trait MessageRouter: EventSink {
    /// Delivers an addressed emission and broadcasts it to the audit sink.
    fn send(&self, emission: Emission);
    /// Delivers a request and returns its correlation ID.
    fn request(&self, emission: Emission) -> CruxId;
    /// Removes the oldest queued emission for an agent.
    fn recv(&self, agent: &str) -> Option<Emission>;
    /// Removes the queued emission matching an agent and correlation ID.
    fn recv_by_correlation(&self, agent: &str, correlation_id: &CruxId) -> Option<Emission>;
}

/// In-memory mailbox router.
pub struct InMemoryRouter {
    sink: Box<dyn EventSink>,
    mailboxes: Mutex<HashMap<String, VecDeque<Emission>>>,
    next_sequence: Mutex<u64>,
}

impl InMemoryRouter {
    /// Creates an empty mailbox router backed by the supplied audit sink.
    pub fn new(sink: Box<dyn EventSink>) -> Self {
        Self {
            sink,
            mailboxes: Mutex::new(HashMap::new()),
            next_sequence: Mutex::new(0),
        }
    }
}

impl EventSink for InMemoryRouter {
    fn emit(&self, event: RuntimeEvent) {
        self.sink.emit(event);
    }
}

impl MessageRouter for InMemoryRouter {
    fn send(&self, emission: Emission) {
        let mut next_sequence = self
            .next_sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.sink.emit(RuntimeEvent {
            sequence: *next_sequence,
            emitted_at: chrono::Utc::now(),
            trace_id: None,
            agent: None,
            emission: emission.clone(),
        });
        *next_sequence = next_sequence.saturating_add(1);
        drop(next_sequence);
        if let Some(recipient) = emission.recipient() {
            let recipient = recipient.to_string();
            let mut mailboxes = self.mailboxes.lock().unwrap();
            mailboxes.entry(recipient).or_default().push_back(emission);
        }
    }

    fn request(&self, mut emission: Emission) -> CruxId {
        let cid = CruxId::new();
        if let Emission::Request {
            ref mut correlation_id,
            ..
        } = emission
        {
            *correlation_id = cid.clone();
        }
        self.send(emission);
        cid
    }

    fn recv(&self, agent: &str) -> Option<Emission> {
        let mut mailboxes = self.mailboxes.lock().unwrap();
        mailboxes.get_mut(agent)?.pop_front()
    }

    fn recv_by_correlation(&self, agent: &str, correlation_id: &CruxId) -> Option<Emission> {
        let mut mailboxes = self.mailboxes.lock().unwrap();
        let queue = mailboxes.get_mut(agent)?;
        let pos = queue
            .iter()
            .position(|e| e.correlation_id() == Some(correlation_id))?;
        queue.remove(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ordered(sequence: u64, emission: Emission) -> RuntimeEvent {
        RuntimeEvent {
            sequence,
            emitted_at: chrono::Utc::now(),
            trace_id: None,
            agent: None,
            emission,
        }
    }

    #[test]
    fn runtime_event_serializes_ordered_flattened_payload() {
        let trace_id = CruxId::new();
        let event = RuntimeEvent {
            sequence: 7,
            emitted_at: chrono::Utc::now(),
            trace_id: Some(trace_id.clone()),
            agent: Some("builder".into()),
            emission: Emission::StepStart {
                name: "compile".into(),
            },
        };

        let value = serde_json::to_value(&event).expect("runtime event should serialize");
        assert_eq!(value["sequence"], 7);
        assert_eq!(value["trace_id"], trace_id.as_str());
        assert_eq!(value["agent"], "builder");
        assert_eq!(value["kind"], "step_start");
        assert_eq!(value["name"], "compile");

        let decoded: RuntimeEvent =
            serde_json::from_value(value).expect("runtime event should deserialize");
        assert_eq!(decoded, event);
    }

    #[test]
    fn runtime_event_filter_selects_replay_and_step_name() {
        let event = RuntimeEvent {
            sequence: 3,
            emitted_at: chrono::Utc::now(),
            trace_id: None,
            agent: None,
            emission: Emission::ReplayHit {
                name: "fetch".into(),
            },
        };

        assert!(event.matches(&RuntimeEventFilter {
            replay_only: true,
            step_name: Some("fetch".into()),
        }));
        assert!(!event.matches(&RuntimeEventFilter {
            replay_only: true,
            step_name: Some("compile".into()),
        }));
    }

    // -- Emission serde --

    #[test]
    fn emission_step_start_round_trip() {
        let e = Emission::StepStart {
            name: "my_step".into(),
        };
        let json_str = serde_json::to_string(&e).unwrap();
        let back: Emission = serde_json::from_str(&json_str).unwrap();
        assert!(matches!(back, Emission::StepStart { ref name } if name == "my_step"));
    }

    #[test]
    fn emission_request_round_trip() {
        let e = Emission::Request {
            sender: "agent_a".into(),
            recipient: "agent_b".into(),
            correlation_id: CruxId::new(),
            payload: json!({"q": "hello"}),
        };
        let json_str = serde_json::to_string(&e).unwrap();
        let back: Emission = serde_json::from_str(&json_str).unwrap();
        assert!(matches!(back, Emission::Request { ref sender, .. } if sender == "agent_a"));
    }

    #[test]
    fn emission_is_addressed() {
        let broadcast = Emission::StepStart { name: "x".into() };
        assert!(!broadcast.is_addressed());

        let addressed = Emission::Message {
            sender: "a".into(),
            recipient: "b".into(),
            payload: json!(null),
        };
        assert!(addressed.is_addressed());
        assert_eq!(addressed.recipient(), Some("b"));
    }

    #[test]
    fn null_sink_does_not_panic() {
        let sink = NullSink;
        sink.emit(ordered(0, Emission::StepStart { name: "x".into() }));
    }

    #[test]
    fn multi_sink_fans_out() {
        struct Counter(Mutex<usize>);
        impl EventSink for Counter {
            fn emit(&self, _event: RuntimeEvent) {
                *self.0.lock().unwrap() += 1;
            }
        }

        let c1 = Arc::new(Counter(Mutex::new(0)));
        let c2 = Arc::new(Counter(Mutex::new(0)));
        let multi = MultiSink::new(vec![c1.clone(), c2.clone()]);
        multi.emit(ordered(0, Emission::StepStart { name: "x".into() }));
        assert_eq!(*c1.0.lock().unwrap(), 1);
        assert_eq!(*c2.0.lock().unwrap(), 1);
    }

    #[test]
    fn jsonl_writer_appends_valid_jsonl() {
        let dir = std::env::temp_dir().join(format!("crux_test_{}", CruxId::new().as_str()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("trace.jsonl");

        let writer = JsonlWriter::new(&path);
        writer.emit(ordered(0, Emission::StepStart { name: "s1".into() }));
        writer.emit(ordered(
            1,
            Emission::StepComplete {
                name: "s1".into(),
                duration_ms: 42,
            },
        ));

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);

        for line in &lines {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }

        let first: RuntimeEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first.sequence, 0);
        assert!(matches!(first.emission, Emission::StepStart { ref name } if name == "s1"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn jsonl_writer_does_not_panic_on_bad_path() {
        let writer = JsonlWriter::new("/nonexistent/dir/trace.jsonl");
        writer.emit(ordered(0, Emission::StepStart { name: "x".into() }));
    }

    #[test]
    fn metrics_sink_exports_prometheus_counters() {
        let sink = MetricsSink::default();
        sink.emit(ordered(0, Emission::StepStart { name: "a".into() }));
        sink.emit(ordered(
            1,
            Emission::StepComplete {
                name: "a".into(),
                duration_ms: 12,
            },
        ));
        sink.emit(ordered(
            2,
            Emission::StepError {
                name: "b".into(),
                error: "boom".into(),
            },
        ));

        let metrics = sink.render_prometheus();
        assert!(metrics.contains("crux_emissions_total 3"));
        assert!(metrics.contains("crux_steps_completed_total 1"));
        assert!(metrics.contains("crux_steps_failed_total 1"));
        assert!(metrics.contains("crux_step_duration_milliseconds_total 12"));
    }

    // -- VectorFileSink --

    #[cfg(feature = "vector-file")]
    #[test]
    fn vector_file_sink_writes_envelope() {
        let dir = std::env::temp_dir().join(format!("crux_vec_{}", CruxId::new().as_str()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vector.jsonl");

        let sink = VectorFileSink::new(&path, "crux-test");
        sink.emit(ordered(0, Emission::StepStart { name: "s1".into() }));

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(parsed["source"], "crux-test");
        assert!(parsed["timestamp"].is_string());
        assert_eq!(parsed["event"]["kind"], "step_start");
        assert_eq!(parsed["event"]["name"], "s1");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(feature = "vector-file")]
    #[test]
    fn vector_file_sink_does_not_panic_on_bad_path() {
        let sink = VectorFileSink::new("/nonexistent/dir/vector.jsonl", "crux");
        sink.emit(ordered(0, Emission::StepStart { name: "x".into() }));
    }

    #[cfg(feature = "vector-http")]
    #[test]
    fn vector_http_sink_constructs_envelope() {
        let sink = VectorHttpSink::new("http://localhost:9999", "crux-test");
        let envelope = sink.envelope(&ordered(
            0,
            Emission::StepComplete {
                name: "s1".into(),
                duration_ms: 42,
            },
        ));
        assert!(envelope.is_some());
        let val = envelope.unwrap();
        assert_eq!(val["source"], "crux-test");
        assert!(val["timestamp"].is_string());
        assert_eq!(val["event"]["kind"], "step_complete");
        assert_eq!(val["event"]["duration_ms"], 42);
    }

    // -- MessageRouter conformance --

    fn assert_router_contract(router: &dyn MessageRouter) {
        // 1. send to agent, recv returns it
        router.send(Emission::Message {
            sender: "a".into(),
            recipient: "b".into(),
            payload: json!(1),
        });
        let msg = router.recv("b");
        assert!(msg.is_some());
        assert!(matches!(msg.unwrap(), Emission::Message { ref sender, .. } if sender == "a"));

        // 2. recv from empty mailbox returns None
        assert!(router.recv("b").is_none());

        // 3. request returns correlation ID, reply is receivable
        let cid = router.request(Emission::Request {
            sender: "a".into(),
            recipient: "b".into(),
            correlation_id: CruxId::new(),
            payload: json!("q"),
        });

        // Simulate reply
        router.send(Emission::Reply {
            sender: "b".into(),
            recipient: "a".into(),
            correlation_id: cid.clone(),
            payload: json!("answer"),
        });

        // 4. recv_by_correlation finds the reply
        let reply = router.recv_by_correlation("a", &cid);
        assert!(reply.is_some());

        // 5. recv_by_correlation ignores non-matching
        assert!(router.recv_by_correlation("a", &CruxId::new()).is_none());

        // 6. FIFO ordering
        router.send(Emission::Message {
            sender: "x".into(),
            recipient: "c".into(),
            payload: json!(1),
        });
        router.send(Emission::Message {
            sender: "x".into(),
            recipient: "c".into(),
            payload: json!(2),
        });
        let first = router.recv("c").unwrap();
        let second = router.recv("c").unwrap();
        assert!(matches!(first, Emission::Message { payload, .. } if payload == json!(1)));
        assert!(matches!(second, Emission::Message { payload, .. } if payload == json!(2)));
    }

    #[test]
    fn in_memory_router_satisfies_contract() {
        let router = InMemoryRouter::new(Box::new(NullSink));
        assert_router_contract(&router);
    }

    #[test]
    fn in_memory_router_emits_to_sink() {
        struct RecordingSink(Mutex<Vec<RuntimeEvent>>);
        impl EventSink for RecordingSink {
            fn emit(&self, event: RuntimeEvent) {
                self.0.lock().unwrap().push(event);
            }
        }

        let recording = Arc::new(RecordingSink(Mutex::new(Vec::new())));
        let router = InMemoryRouter::new(Box::new(Arc::clone(&recording)));
        router.send(Emission::Message {
            sender: "a".into(),
            recipient: "b".into(),
            payload: json!(null),
        });
        assert_eq!(recording.0.lock().unwrap().len(), 1);
    }
}
