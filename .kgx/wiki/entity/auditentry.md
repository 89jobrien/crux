---
title: AuditEntry
tags: [type, runtime, audit]
---
# AuditEntry
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/audit.rs`

Fields: timestamp(f64), agent_id, tool_name, action("allowed"/"denied"/"review"/"error"),
policy_name, details(HashMap<String, String>). Recorded via [[AuditSink]].
