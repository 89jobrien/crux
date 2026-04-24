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
