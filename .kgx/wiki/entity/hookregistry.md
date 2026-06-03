---
title: HookRegistry
tags: [type, runtime, hooks]
---
# HookRegistry
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/hooks.rs`

Stores and invokes scoped lifecycle hooks. [[CruxCtx]] delegates here.
Supports: pre-step gates ([[HookVerdict]]), low-confidence handlers,
step-failure handlers, budget-exceeded handlers. Handlers return [[Recovery<T>]].
