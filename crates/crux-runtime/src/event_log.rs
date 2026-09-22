//! Append-only durable log for typed runtime events.

use crux_domain::event::StepEvent;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggedEvent {
    pub sequence: u64,
    pub event: StepEvent,
}

#[derive(Debug, thiserror::Error)]
pub enum EventLogError {
    #[error("event log I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("event log serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub struct EventLog {
    path: std::path::PathBuf,
    next_sequence: std::sync::Mutex<u64>,
}

impl EventLog {
    pub fn open(path: impl Into<std::path::PathBuf>) -> Self {
        let path = path.into();
        let next_sequence = std::fs::File::open(&path)
            .ok()
            .map(|file| std::io::BufReader::new(file).lines().count() as u64)
            .unwrap_or(0);
        Self {
            path,
            next_sequence: std::sync::Mutex::new(next_sequence),
        }
    }

    pub fn append(&self, event: &StepEvent) -> Result<u64, EventLogError> {
        let mut sequence = self
            .next_sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = LoggedEvent {
            sequence: *sequence,
            event: event.clone(),
        };
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        serde_json::to_writer(&mut file, &entry)?;
        writeln!(file)?;
        *sequence += 1;
        Ok(entry.sequence)
    }

    pub fn replay(&self) -> Result<Vec<LoggedEvent>, EventLogError> {
        let file = std::fs::File::open(&self.path)?;
        std::io::BufReader::new(file)
            .lines()
            .map(|line| Ok(serde_json::from_str(&line?)?))
            .collect()
    }
}
