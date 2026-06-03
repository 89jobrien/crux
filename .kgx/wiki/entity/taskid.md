---
title: TaskId
tags: [type, identity]
---
# TaskId
**Crate:** [[crux-types]] | **File:** `crates/crux-types/src/id.rs`

ULID-based unique task identifier. Prefix `task_`. Used in [[TaskRegistry]] and [[Task]].
Methods: `new()`, `as_str()`, `FromStr`. Implements Display, Hash, Serialize/Deserialize.
