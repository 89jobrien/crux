//! Human or policy approval port for risk-gated changes.
//!
//! Wire types (`RiskLevel`, `ApprovalRequest`, `ApprovalDecision`) live in
//! `crux_types::approval` and are re-exported here for backwards compatibility.

pub use crux_types::approval::{ApprovalDecision, ApprovalRequest, RiskLevel};

use std::future::Future;

/// Port: gates escalation requests (human-in-the-loop or policy engine).
pub trait ApprovalGate: Send + Sync {
    /// Evaluates a risk-classified change request and returns a gate decision.
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
