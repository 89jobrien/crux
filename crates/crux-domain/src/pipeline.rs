//! Ordered runtime event ingestion and broadcast fan-out.
//!
//! Architecture:
//!   `EventSender` (cloneable) → sinks → tokio broadcast channel → `EventReceiver`s
//!
//! The broadcast channel gives each subscriber its own view of the event stream.
//! Lagging receivers silently drop events (broadcast semantics) — consumers
//! that need guaranteed delivery should use a separate MPSC tap.
use std::sync::{Arc, Mutex};

use crux_types::emission::{Emission, EventSink, RuntimeEvent};
use crux_types::id::CruxId;
use tokio::sync::broadcast;

use crate::event::StepEvent;

/// Error returned when an event is emitted without an active subscriber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventSendError;

impl std::fmt::Display for EventSendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ordered event stream has no active subscribers")
    }
}

impl std::error::Error for EventSendError {}

struct Publisher {
    next_sequence: u64,
    tx: broadcast::Sender<RuntimeEvent>,
    sinks: Vec<Arc<dyn EventSink>>,
}

/// A cloneable sender that serializes sequence assignment and delivery.
#[derive(Clone)]
pub struct EventSender {
    publisher: Arc<Mutex<Publisher>>,
}

impl EventSender {
    /// Emits a compatibility `StepEvent` into the canonical ordered stream.
    pub fn send(&self, event: StepEvent) -> Result<usize, EventSendError> {
        let (_, result) = self.publish(None, None, event.into_emission());
        result
    }

    /// Emits a canonical payload with optional trace context.
    pub fn emit(
        &self,
        trace_id: Option<CruxId>,
        agent: Option<&str>,
        emission: Emission,
    ) -> RuntimeEvent {
        self.publish(trace_id, agent, emission).0
    }

    fn publish(
        &self,
        trace_id: Option<CruxId>,
        agent: Option<&str>,
        emission: Emission,
    ) -> (RuntimeEvent, Result<usize, EventSendError>) {
        let mut publisher = self
            .publisher
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let event = RuntimeEvent {
            sequence: publisher.next_sequence,
            emitted_at: chrono::Utc::now(),
            trace_id,
            agent: agent.map(str::to_owned),
            emission,
        };
        publisher.next_sequence = publisher.next_sequence.saturating_add(1);
        for sink in &publisher.sinks {
            sink.emit(event.clone());
        }
        let result = publisher.tx.send(event.clone()).map_err(|_| EventSendError);
        (event, result)
    }
}

/// A receiver handle for consuming step events.
pub type EventReceiver = broadcast::Receiver<RuntimeEvent>;

/// The event pipeline — owns the broadcast channel.
///
/// Call `subscribe()` before emitting events to avoid missing early events.
pub struct EventPipeline {
    publisher: Arc<Mutex<Publisher>>,
}

impl EventPipeline {
    /// Create a new pipeline with the given broadcast buffer capacity.
    ///
    /// `capacity` is the number of events buffered per subscriber. Lagging
    /// subscribers will miss events once the buffer fills.
    pub fn new(capacity: usize) -> Self {
        Self::with_sinks(capacity, Vec::new())
    }

    /// Creates a pipeline that delivers each ordered event to the supplied sinks.
    pub fn with_sinks(capacity: usize, sinks: Vec<Arc<dyn EventSink>>) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            publisher: Arc::new(Mutex::new(Publisher {
                next_sequence: 0,
                tx,
                sinks,
            })),
        }
    }

    /// Get a cloneable sender for emitting events.
    pub fn sender(&self) -> EventSender {
        EventSender {
            publisher: Arc::clone(&self.publisher),
        }
    }

    /// Subscribe to the event stream. Receives events emitted after this call.
    pub fn subscribe(&self) -> EventReceiver {
        self.publisher
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tx
            .subscribe()
    }
}
