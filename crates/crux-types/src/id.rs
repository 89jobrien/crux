/// Unique identifiers for crux traces and tasks.
use serde::{Deserialize, Serialize};
use std::fmt;
use ulid::Ulid;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CruxId(String);

impl CruxId {
    pub fn new() -> Self {
        Self(format!("crux_{}", Ulid::new()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for CruxId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for CruxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(String);

impl TaskId {
    pub fn new() -> Self {
        Self(format!("task_{}", Ulid::new()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for TaskId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crux_id_has_prefix() {
        let id = CruxId::new();
        assert!(id.as_str().starts_with("crux_"));
    }

    #[test]
    fn task_id_has_prefix() {
        let id = TaskId::new();
        assert!(id.as_str().starts_with("task_"));
    }

    #[test]
    fn ids_are_unique() {
        let a = CruxId::new();
        let b = CruxId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn serde_round_trip() {
        let id = CruxId::new();
        let json = serde_json::to_string(&id).unwrap();
        let back: CruxId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
