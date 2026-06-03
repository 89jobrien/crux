---
title: RecoveryKind
tags: [type, core]
---
# RecoveryKind
**Crate:** [[crux-types]] | **File:** `crates/crux-types/src/recovery.rs`

Serializable subset of [[Recovery<T>]] (no closures): Retry, Skip, Propagate, Continue.
Used for wire-format persistence where closure variants cannot be serialized.
