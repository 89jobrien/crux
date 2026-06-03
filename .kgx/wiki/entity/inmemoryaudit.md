---
title: InMemoryAudit
tags: [type, runtime, adapter, audit]
---
# InMemoryAudit
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/audit.rs`

In-memory adapter implementing [[AuditSink]]. Methods: `entries()`, `denied()`,
`by_agent(id)`. Used for testing governance flows.
