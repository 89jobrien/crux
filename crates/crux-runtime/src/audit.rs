use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single audit log entry recording a governance decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: f64,
    pub agent_id: String,
    pub tool_name: String,
    /// One of "allowed", "denied", "review", "error".
    pub action: String,
    pub policy_name: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub details: HashMap<String, String>,
}

/// Port: append-only audit log for governance decisions.
pub trait AuditSink: Send + Sync {
    /// Record a single audit entry.
    fn record(&self, entry: AuditEntry);
}

/// In-memory audit trail for testing and short-lived sessions.
pub struct InMemoryAudit {
    entries: std::sync::Mutex<Vec<AuditEntry>>,
}

impl InMemoryAudit {
    pub fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries.lock().unwrap().clone()
    }

    pub fn denied(&self) -> Vec<AuditEntry> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.action == "denied")
            .cloned()
            .collect()
    }

    pub fn by_agent(&self, agent_id: &str) -> Vec<AuditEntry> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.agent_id == agent_id)
            .cloned()
            .collect()
    }
}

impl Default for InMemoryAudit {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditSink for InMemoryAudit {
    fn record(&self, entry: AuditEntry) {
        self.entries.lock().unwrap().push(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(action: &str, agent: &str, tool: &str) -> AuditEntry {
        AuditEntry {
            timestamp: 1000.0,
            agent_id: agent.into(),
            tool_name: tool.into(),
            action: action.into(),
            policy_name: "test".into(),
            details: HashMap::new(),
        }
    }

    #[test]
    fn record_and_retrieve() {
        let audit = InMemoryAudit::new();
        audit.record(entry("allowed", "a1", "search"));
        audit.record(entry("denied", "a1", "shell"));
        assert_eq!(audit.entries().len(), 2);
    }

    #[test]
    fn filter_denied() {
        let audit = InMemoryAudit::new();
        audit.record(entry("allowed", "a1", "search"));
        audit.record(entry("denied", "a1", "shell"));
        audit.record(entry("denied", "a2", "delete"));
        let denied = audit.denied();
        assert_eq!(denied.len(), 2);
        assert!(denied.iter().all(|e| e.action == "denied"));
    }

    #[test]
    fn filter_by_agent() {
        let audit = InMemoryAudit::new();
        audit.record(entry("allowed", "a1", "search"));
        audit.record(entry("denied", "a2", "shell"));
        audit.record(entry("allowed", "a1", "read"));
        let a1 = audit.by_agent("a1");
        assert_eq!(a1.len(), 2);
        assert!(a1.iter().all(|e| e.agent_id == "a1"));
    }

    #[test]
    fn serde_round_trip() {
        let e = AuditEntry {
            timestamp: 1234.5,
            agent_id: "agent-1".into(),
            tool_name: "search".into(),
            action: "allowed".into(),
            policy_name: "prod".into(),
            details: [("duration_ms".into(), "42".into())].into_iter().collect(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: AuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.agent_id, "agent-1");
        assert_eq!(back.details.get("duration_ms").unwrap(), "42");
    }

    #[test]
    fn empty_details_omitted_in_json() {
        let e = entry("allowed", "a1", "search");
        let json = serde_json::to_string(&e).unwrap();
        assert!(!json.contains("details"));
    }
}
