---
title: ApprovalGate
tags: [trait, runtime, port, approval]
---
# ApprovalGate
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/approval.rs`

Trait for human-in-the-loop escalation. Method: `request_approval(req: &ApprovalRequest)`
→ [[ApprovalDecision]]. Adapters: [[AutoApproveGate]], [[TerminalApprovalGate]].
