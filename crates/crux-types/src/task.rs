//! Wire-format types for project task management.

use serde::{Deserialize, Serialize};

/// Task priority levels, ordered by urgency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    P0,
    P1,
    P2,
    P3,
}

/// A freeform label for categorizing tasks.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskLabel(pub String);

/// The kind of relationship between two tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    BlockedBy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_ordering() {
        assert!(Priority::P0 < Priority::P1);
        assert!(Priority::P1 < Priority::P2);
        assert!(Priority::P2 < Priority::P3);
    }

    #[test]
    fn priority_serde_round_trip() {
        let p = Priority::P1;
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "\"p1\"");
        let back: Priority = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Priority::P1);
    }

    #[test]
    fn task_label_serde_round_trip() {
        let label = TaskLabel("backend".into());
        let json = serde_json::to_string(&label).unwrap();
        let back: TaskLabel = serde_json::from_str(&json).unwrap();
        assert_eq!(back, label);
    }

    #[test]
    fn dependency_kind_serde() {
        let kind = DependencyKind::BlockedBy;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"blocked_by\"");
        let back: DependencyKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, DependencyKind::BlockedBy);
    }
}
