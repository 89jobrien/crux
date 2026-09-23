//! Append-only durable log for typed runtime events.

use crux_types::emission::{EventSink, RuntimeEvent, RuntimeEventFilter};
use std::io::{BufRead, Write};

/// Backward-compatible name for an event persisted in the runtime log.
pub type LoggedEvent = RuntimeEvent;

#[derive(Debug, thiserror::Error)]
pub enum EventLogError {
    #[error("event log I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("event log serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub struct EventLog {
    path: std::path::PathBuf,
    append_lock: std::sync::Mutex<()>,
}

impl EventLog {
    /// Opens an append-only JSONL event log.
    pub fn open(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            path: path.into(),
            append_lock: std::sync::Mutex::new(()),
        }
    }

    /// Appends an already ordered event without changing its sequence.
    pub fn append(&self, event: &RuntimeEvent) -> Result<(), EventLogError> {
        let _guard = self
            .append_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        serde_json::to_writer(&mut file, event)?;
        writeln!(file)?;
        Ok(())
    }

    /// Replays all events in append order.
    pub fn replay(&self) -> Result<Vec<RuntimeEvent>, EventLogError> {
        let file = std::fs::File::open(&self.path)?;
        std::io::BufReader::new(file)
            .lines()
            .map(|line| Ok(serde_json::from_str(&line?)?))
            .collect()
    }

    /// Replays only events that satisfy the supplied filter.
    pub fn replay_filtered(
        &self,
        filter: &RuntimeEventFilter,
    ) -> Result<Vec<RuntimeEvent>, EventLogError> {
        Ok(self
            .replay()?
            .into_iter()
            .filter(|event| event.matches(filter))
            .collect())
    }
}

impl EventSink for EventLog {
    fn emit(&self, event: RuntimeEvent) {
        let _ = self.append(&event);
    }
}
