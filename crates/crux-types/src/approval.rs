//! Wire types for the human-or-policy approval port.
//!
//! The `ApprovalGate` trait lives in `crux-runtime` (it uses async / `Future`).
//! This module holds only the serialisable data types that cross crate boundaries.

use serde::{Deserialize, Serialize};

/// How risky a proposed change is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// A request sent to the approval gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub summary: String,
    pub diff_description: String,
    pub risk_level: RiskLevel,
}

/// The gate's response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum ApprovalDecision {
    Approved,
    Denied { reason: String },
    Deferred { timeout_seconds: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_level_serde_round_trip() {
        for level in [
            RiskLevel::Low,
            RiskLevel::Medium,
            RiskLevel::High,
            RiskLevel::Critical,
        ] {
            let json = serde_json::to_string(&level).expect("serialise");
            let back: RiskLevel = serde_json::from_str(&json).expect("deserialise");
            assert_eq!(back, level);
        }
    }

    #[test]
    fn approval_decision_approved_round_trip() {
        let decision = ApprovalDecision::Approved;
        let json = serde_json::to_string(&decision).expect("serialise");
        let back: ApprovalDecision = serde_json::from_str(&json).expect("deserialise");
        assert!(matches!(back, ApprovalDecision::Approved));
    }

    #[test]
    fn approval_decision_denied_carries_reason() {
        let decision = ApprovalDecision::Denied {
            reason: "violates syscall policy".into(),
        };
        let json = serde_json::to_string(&decision).expect("serialise");
        let back: ApprovalDecision = serde_json::from_str(&json).expect("deserialise");
        if let ApprovalDecision::Denied { reason } = back {
            assert!(reason.contains("syscall"));
        } else {
            panic!("expected Denied");
        }
    }

    #[test]
    fn approval_decision_deferred_round_trip() {
        let decision = ApprovalDecision::Deferred {
            timeout_seconds: 3600,
        };
        let json = serde_json::to_string(&decision).expect("serialise");
        let back: ApprovalDecision = serde_json::from_str(&json).expect("deserialise");
        if let ApprovalDecision::Deferred { timeout_seconds } = back {
            assert_eq!(timeout_seconds, 3600);
        } else {
            panic!("expected Deferred");
        }
    }

    #[test]
    fn approval_request_serde_round_trip() {
        let req = ApprovalRequest {
            summary: "enable network access".into(),
            diff_description: "network_access: false -> true".into(),
            risk_level: RiskLevel::Medium,
        };
        let json = serde_json::to_string(&req).expect("serialise");
        let back: ApprovalRequest = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back.summary, "enable network access");
        assert_eq!(back.risk_level, RiskLevel::Medium);
    }
}
