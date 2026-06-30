//! Emission — the unified event type for observability and agent messaging.
//!
//! `Emission` is the single type that flows through every `EventSink`.
//! It unifies step lifecycle, combinator lifecycle, runtime internals,
//! and agent-to-agent messaging into one enum.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::id::CruxId;

/// The single event type that flows through every EventSink.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

// ---------------------------------------------------------------------------
// EventSink trait + adapters
// ---------------------------------------------------------------------------

/// Write-only broadcast port. Implementations must be non-fatal —
/// a failed write must never abort the calling workflow.
pub trait EventSink: Send + Sync {
    fn emit(&self, emission: Emission);
}

impl<T: EventSink> EventSink for Arc<T> {
    fn emit(&self, emission: Emission) {
        (**self).emit(emission);
    }
}

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
    pub fn new(sinks: Vec<Arc<dyn EventSink>>) -> Self {
        Self { sinks }
    }
}

impl EventSink for MultiSink {
    fn emit(&self, emission: Emission) {
        for sink in &self.sinks {
            sink.emit(emission.clone());
        }
    }
}

/// Appends Emission as JSON lines to a file. Non-fatal on I/O error.
pub struct JsonlWriter {
    path: std::path::PathBuf,
}

impl JsonlWriter {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl EventSink for JsonlWriter {
    fn emit(&self, emission: Emission) {
        use std::io::Write;
        let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            return;
        };
        let Ok(json) = serde_json::to_string(&emission) else {
            return;
        };
        let _ = writeln!(file, "{json}");
    }
}

// ---------------------------------------------------------------------------
// VectorFileSink — JSONL with Vector-friendly envelope (timestamp + source)
// ---------------------------------------------------------------------------

/// Writes Emission as JSONL with a Vector-friendly envelope.
/// Each line contains `{ "timestamp": ..., "source": "crux", "event": { ... } }`.
/// Configure Vector's `file` source to tail this path.
#[cfg(feature = "vector-file")]
pub struct VectorFileSink {
    path: std::path::PathBuf,
    source_label: String,
}

#[cfg(feature = "vector-file")]
impl VectorFileSink {
    pub fn new(path: impl Into<std::path::PathBuf>, source_label: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            source_label: source_label.into(),
        }
    }

    fn envelope(&self, emission: &Emission) -> Option<String> {
        let event = serde_json::to_value(emission).ok()?;
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
    fn emit(&self, emission: Emission) {
        use std::io::Write;
        let Some(line) = self.envelope(&emission) else {
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

// ---------------------------------------------------------------------------
// VectorHttpSink — POST JSON to Vector's HTTP source
// ---------------------------------------------------------------------------

/// Sends Emission as JSON to Vector's `http` source endpoint.
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
    pub fn new(url: impl Into<String>, source_label: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            source_label: source_label.into(),
            client: reqwest::Client::new(),
        }
    }

    fn envelope(&self, emission: &Emission) -> Option<serde_json::Value> {
        let event = serde_json::to_value(emission).ok()?;
        Some(serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "source": self.source_label,
            "event": event,
        }))
    }
}

#[cfg(feature = "vector-http")]
impl EventSink for VectorHttpSink {
    fn emit(&self, emission: Emission) {
        let Some(body) = self.envelope(&emission) else {
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

// ---------------------------------------------------------------------------
// MessageRouter trait + InMemoryRouter
// ---------------------------------------------------------------------------

/// Addressed request-reply port. Every send/request also emits
/// to the underlying EventSink for auditability.
pub trait MessageRouter: EventSink {
    fn send(&self, emission: Emission);
    fn request(&self, emission: Emission) -> CruxId;
    fn recv(&self, agent: &str) -> Option<Emission>;
    fn recv_by_correlation(&self, agent: &str, correlation_id: &CruxId) -> Option<Emission>;
}

/// In-memory mailbox router.
pub struct InMemoryRouter {
    sink: Box<dyn EventSink>,
    mailboxes: Mutex<HashMap<String, VecDeque<Emission>>>,
}

impl InMemoryRouter {
    pub fn new(sink: Box<dyn EventSink>) -> Self {
        Self {
            sink,
            mailboxes: Mutex::new(HashMap::new()),
        }
    }
}

impl EventSink for InMemoryRouter {
    fn emit(&self, emission: Emission) {
        self.sink.emit(emission);
    }
}

impl MessageRouter for InMemoryRouter {
    fn send(&self, emission: Emission) {
        self.sink.emit(emission.clone());
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    // -- NullSink --

    #[test]
    fn null_sink_does_not_panic() {
        let sink = NullSink;
        sink.emit(Emission::StepStart { name: "x".into() });
    }

    // -- MultiSink --

    #[test]
    fn multi_sink_fans_out() {
        struct Counter(Mutex<usize>);
        impl EventSink for Counter {
            fn emit(&self, _emission: Emission) {
                *self.0.lock().unwrap() += 1;
            }
        }

        let c1 = Arc::new(Counter(Mutex::new(0)));
        let c2 = Arc::new(Counter(Mutex::new(0)));
        let multi = MultiSink::new(vec![c1.clone(), c2.clone()]);
        multi.emit(Emission::StepStart { name: "x".into() });
        assert_eq!(*c1.0.lock().unwrap(), 1);
        assert_eq!(*c2.0.lock().unwrap(), 1);
    }

    // -- JsonlWriter --

    #[test]
    fn jsonl_writer_appends_valid_jsonl() {
        let dir = std::env::temp_dir().join(format!("crux_test_{}", CruxId::new().as_str()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("trace.jsonl");

        let writer = JsonlWriter::new(&path);
        writer.emit(Emission::StepStart { name: "s1".into() });
        writer.emit(Emission::StepComplete {
            name: "s1".into(),
            duration_ms: 42,
        });

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);

        for line in &lines {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }

        let first: Emission = serde_json::from_str(lines[0]).unwrap();
        assert!(matches!(first, Emission::StepStart { ref name } if name == "s1"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn jsonl_writer_does_not_panic_on_bad_path() {
        let writer = JsonlWriter::new("/nonexistent/dir/trace.jsonl");
        writer.emit(Emission::StepStart { name: "x".into() });
    }

    // -- VectorFileSink --

    #[cfg(feature = "vector-file")]
    #[test]
    fn vector_file_sink_writes_envelope() {
        let dir = std::env::temp_dir().join(format!("crux_vec_{}", CruxId::new().as_str()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vector.jsonl");

        let sink = VectorFileSink::new(&path, "crux-test");
        sink.emit(Emission::StepStart { name: "s1".into() });

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
        sink.emit(Emission::StepStart { name: "x".into() });
    }

    // -- VectorHttpSink --

    #[cfg(feature = "vector-http")]
    #[test]
    fn vector_http_sink_constructs_envelope() {
        let sink = VectorHttpSink::new("http://localhost:9999", "crux-test");
        let envelope = sink.envelope(&Emission::StepComplete {
            name: "s1".into(),
            duration_ms: 42,
        });
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
        struct RecordingSink(Mutex<Vec<Emission>>);
        impl EventSink for RecordingSink {
            fn emit(&self, emission: Emission) {
                self.0.lock().unwrap().push(emission);
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
