use cruxx_agentic::adapters::terminal_approval::AutoApproveGate;
use cruxx_core::approval::{ApprovalDecision, ApprovalGate, ApprovalRequest, RiskLevel};

#[tokio::test]
async fn auto_approve_gate_approves_low_risk() {
    let gate = AutoApproveGate::new(RiskLevel::Medium);
    let req = ApprovalRequest {
        summary: "bump memory".into(),
        diff_description: "memory_mb: 512 -> 768".into(),
        risk_level: RiskLevel::Low,
    };
    let decision = gate.request_approval(&req).await;
    assert!(matches!(decision, ApprovalDecision::Approved));
}

#[tokio::test]
async fn auto_approve_gate_denies_above_threshold() {
    let gate = AutoApproveGate::new(RiskLevel::Low);
    let req = ApprovalRequest {
        summary: "enable network".into(),
        diff_description: "network: false -> true".into(),
        risk_level: RiskLevel::Medium,
    };
    let decision = gate.request_approval(&req).await;
    assert!(matches!(decision, ApprovalDecision::Denied { .. }));
}
