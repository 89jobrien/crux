use cruxx_core::approval::{ApprovalDecision, ApprovalGate, ApprovalRequest, RiskLevel};

/// Auto-approve gate that approves anything at or below the configured risk threshold.
/// For testing and non-interactive environments.
pub struct AutoApproveGate {
    max_auto_approve: RiskLevel,
}

impl AutoApproveGate {
    pub fn new(max_auto_approve: RiskLevel) -> Self {
        Self { max_auto_approve }
    }
}

fn risk_severity(level: RiskLevel) -> u8 {
    match level {
        RiskLevel::Low => 1,
        RiskLevel::Medium => 2,
        RiskLevel::High => 3,
        RiskLevel::Critical => 4,
    }
}

impl ApprovalGate for AutoApproveGate {
    async fn request_approval(&self, request: &ApprovalRequest) -> ApprovalDecision {
        if risk_severity(request.risk_level) <= risk_severity(self.max_auto_approve) {
            ApprovalDecision::Approved
        } else {
            ApprovalDecision::Denied {
                reason: format!(
                    "risk level {:?} exceeds auto-approve threshold {:?}",
                    request.risk_level, self.max_auto_approve
                ),
            }
        }
    }
}

/// Interactive terminal gate — prints the request and reads y/n from stdin.
/// Not usable in tests; use `AutoApproveGate` for testing.
pub struct TerminalApprovalGate;

impl ApprovalGate for TerminalApprovalGate {
    async fn request_approval(&self, request: &ApprovalRequest) -> ApprovalDecision {
        eprintln!("--- APPROVAL REQUIRED ---");
        eprintln!("Summary: {}", request.summary);
        eprintln!("Risk: {:?}", request.risk_level);
        eprintln!("Diff: {}", request.diff_description);
        eprintln!("Approve? [y/N]: ");

        let answer = tokio::task::spawn_blocking(|| {
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).ok();
            buf.trim().to_lowercase()
        })
        .await
        .unwrap_or_default();

        if answer == "y" || answer == "yes" {
            ApprovalDecision::Approved
        } else {
            ApprovalDecision::Denied {
                reason: "user denied".into(),
            }
        }
    }
}
