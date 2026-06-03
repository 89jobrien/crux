---
title: Governance and Safety
source_document: crux_runtime_crate
tags: [governance, safety, trust, approval]
---

# Governance and Safety

## Trust
- [[TrustScore]] -- per-agent with temporal decay: record_success/failure,
  current(decay_rate), reliability()
- [[TrustRegistry]] -- maps agent IDs → TrustScore, most_trusted(),
  meets_threshold()

## Governance
- [[GovernancePolicy]] -- composable: allowed/blocked tools, blocked patterns,
  rate limits, require_human_approval list
- [[PolicyAction]] -- Allow / Deny / Review
- `compose_policies()` -- union blocked, intersect allowed, min rate limits

## Safety
- [[SafetyPolicy]] trait -- validate(diff, base), requires_approval(diff)
- [[SafetyViolation]] -- HardCapExceeded / ForbiddenSyscall / Custom

## Approval
- [[ApprovalGate]] trait -- request_approval(req) → [[ApprovalDecision]]
- [[ApprovalDecision]] -- Approved / Denied{reason} / Deferred{timeout}
- [[RiskLevel]] -- Low / Medium / High / Critical
- Adapters: [[AutoApproveGate]] (up to threshold), [[TerminalApprovalGate]] (stdin)

## Audit
- [[AuditSink]] trait -- record(entry)
- [[AuditEntry]] -- timestamp, agent, tool, action, policy, details
- [[InMemoryAudit]] -- test adapter with filtered queries
