use serde::{Deserialize, Serialize};
use std::future::Future;

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

/// Port: gates escalation requests (human-in-the-loop or policy engine).
pub trait ApprovalGate: Send + Sync {
    fn request_approval(
        &self,
        request: &ApprovalRequest,
    ) -> impl Future<Output = ApprovalDecision> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysApprove;

    impl ApprovalGate for AlwaysApprove {
        async fn request_approval(&self, request: &ApprovalRequest) -> ApprovalDecision {
            let _ = request;
            ApprovalDecision::Approved
        }
    }

    struct AlwaysDeny;

    impl ApprovalGate for AlwaysDeny {
        async fn request_approval(&self, _request: &ApprovalRequest) -> ApprovalDecision {
            ApprovalDecision::Denied {
                reason: "policy".into(),
            }
        }
    }

    #[tokio::test]
    async fn approve_gate_returns_approved() {
        let gate = AlwaysApprove;
        let req = ApprovalRequest {
            summary: "enable network access".into(),
            diff_description: "network_access: false -> true".into(),
            risk_level: RiskLevel::Medium,
        };
        assert!(matches!(
            gate.request_approval(&req).await,
            ApprovalDecision::Approved
        ));
    }

    #[tokio::test]
    async fn deny_gate_returns_denied() {
        let gate = AlwaysDeny;
        let req = ApprovalRequest {
            summary: "add dangerous syscall".into(),
            diff_description: "syscalls += ptrace".into(),
            risk_level: RiskLevel::High,
        };
        assert!(matches!(
            gate.request_approval(&req).await,
            ApprovalDecision::Denied { .. }
        ));
    }
}
