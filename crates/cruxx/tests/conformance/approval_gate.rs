/// Conformance tests: ApprovalGate port — contract verification via test-local adapters.
///
/// Verifies AlwaysApprove / AlwaysDeny adapters satisfy port contract and that
/// ApprovalRequest / ApprovalDecision serde contracts are stable.
use cruxx::prelude::{ApprovalDecision, ApprovalGate, ApprovalRequest, RiskLevel};

// ── Test-local adapters ──────────────────────────────────────────────────────

struct AlwaysApprove;

impl ApprovalGate for AlwaysApprove {
    async fn request_approval(&self, _request: &ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Approved
    }
}

struct AlwaysDeny;

impl ApprovalGate for AlwaysDeny {
    async fn request_approval(&self, _request: &ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Denied {
            reason: "policy denied".into(),
        }
    }
}

fn sample_request() -> ApprovalRequest {
    ApprovalRequest {
        summary: "enable network access".into(),
        diff_description: "network_access: false -> true".into(),
        risk_level: RiskLevel::Medium,
    }
}

// ── Adapter behaviour ────────────────────────────────────────────────────────

#[tokio::test]
async fn conformance_approval_gate_always_approve_returns_approved() {
    let gate = AlwaysApprove;
    let decision = gate.request_approval(&sample_request()).await;
    assert!(
        matches!(decision, ApprovalDecision::Approved),
        "expected Approved, got {decision:?}"
    );
}

#[tokio::test]
async fn conformance_approval_gate_always_deny_returns_denied_with_reason() {
    let gate = AlwaysDeny;
    let decision = gate.request_approval(&sample_request()).await;
    assert!(
        matches!(decision, ApprovalDecision::Denied { .. }),
        "expected Denied, got {decision:?}"
    );
    if let ApprovalDecision::Denied { reason } = decision {
        assert!(!reason.is_empty(), "Denied reason must not be empty");
    }
}

// ── Serde contracts ──────────────────────────────────────────────────────────

#[test]
fn conformance_approval_gate_request_serde_roundtrip() {
    let req = sample_request();
    let json = serde_json::to_string(&req).expect("serialize");
    let back: ApprovalRequest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.summary, req.summary);
    assert_eq!(back.diff_description, req.diff_description);
    assert!(matches!(back.risk_level, RiskLevel::Medium));
}

#[test]
fn conformance_approval_gate_decision_approved_serializes_with_tag() {
    let decision = ApprovalDecision::Approved;
    let json = serde_json::to_string(&decision).expect("serialize");
    assert!(
        json.contains(r#""decision":"approved""#),
        "expected tagged 'approved', got: {json}"
    );
}

#[test]
fn conformance_approval_gate_decision_denied_serializes_with_tag_and_reason() {
    let decision = ApprovalDecision::Denied {
        reason: "blocked by policy".into(),
    };
    let json = serde_json::to_string(&decision).expect("serialize");
    assert!(
        json.contains(r#""decision":"denied""#),
        "expected tagged 'denied', got: {json}"
    );
    assert!(
        json.contains("reason"),
        "expected 'reason' field in JSON, got: {json}"
    );
}

#[test]
fn conformance_approval_gate_decision_deferred_contains_timeout() {
    let decision = ApprovalDecision::Deferred {
        timeout_seconds: 300,
    };
    let json = serde_json::to_string(&decision).expect("serialize");
    assert!(
        json.contains("timeout_seconds"),
        "expected 'timeout_seconds' field, got: {json}"
    );
    assert!(
        json.contains(r#""decision":"deferred""#),
        "ApprovalDecision::Deferred must serialize decision tag as 'deferred', got: {json}"
    );
    let back: ApprovalDecision = serde_json::from_str(&json).expect("deserialize");
    assert!(
        matches!(
            back,
            ApprovalDecision::Deferred {
                timeout_seconds: 300
            }
        ),
        "expected Deferred(300), got {back:?}"
    );
}
