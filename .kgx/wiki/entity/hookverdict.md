---
title: HookVerdict
tags: [type, runtime, hooks]
---
# HookVerdict
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/hooks.rs`

Enum: Allow or Deny(String). Returned by pre-step gate closures registered
on [[HookRegistry]]. If Deny, step is blocked.
