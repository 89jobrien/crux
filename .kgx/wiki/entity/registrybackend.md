---
title: RegistryBackend
tags: [trait, runtime, port, registry]
---
# RegistryBackend
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/registry/backend.rs`

Trait (Send + Sync) for task storage. Methods: `get(id)`, `put(id, data)`,
`list(prefix)`, `cas(id, expected, new)` (compare-and-set).
Adapters: [[InMemoryBackend]], [[RedbBackend]].
