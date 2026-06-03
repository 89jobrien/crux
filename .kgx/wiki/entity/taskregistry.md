---
title: TaskRegistry
tags: [type, runtime, registry]
---
# TaskRegistry
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/registry/task.rs`

`TaskRegistry<B: RegistryBackend>` -- typed API for task lifecycle.
Methods: `submit(kind, input)` → [[TaskId]], `get(id)` → [[Task]],
`update_status(id, status)` (CAS-based), `checkpoint(id, snapshot)`,
`load_checkpoint(id)`, `pending(prefix)`, `resume(id)`.
