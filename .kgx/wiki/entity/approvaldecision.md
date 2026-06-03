---
title: ApprovalDecision
tags: [type, runtime, approval]
---
# ApprovalDecision
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/approval.rs`

Enum: Approved, Denied{reason: String}, Deferred{timeout_seconds: u64}.
Returned by [[ApprovalGate]]::request_approval().
