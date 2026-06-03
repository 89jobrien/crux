---
title: Recovery<T>
tags: [type, runtime, hooks]
---
# Recovery<T>
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/types/recovery.rs`

Hook return type for lifecycle handlers. Variants:
- `Retry` -- re-run same step
- `RetryWith(Box<dyn FnOnce() -> BoxFut<T>>)` -- re-run with different closure
- `Substitute(T)` -- use this value instead
- `Escalate(BoxFut<T>)` -- run future as escalation path
- `Propagate` -- let error propagate
- `Skip` -- mark as skipped, continue
- `Continue` -- ignore low confidence

Serializable subset: [[RecoveryKind]].
