---
title: AuditSink
tags: [trait, runtime, port, audit]
---
# AuditSink
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/audit.rs`

Trait (Send + Sync) for append-only audit logging. Method: `record(entry: [[AuditEntry]])`.
Adapter: [[InMemoryAudit]] for testing.
