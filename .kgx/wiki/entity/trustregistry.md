---
title: TrustRegistry
tags: [type, runtime, trust]
---
# TrustRegistry
**Crate:** [[crux-runtime]] | **File:** `crates/crux-runtime/src/trust.rs`

Maps agent IDs to [[TrustScore]]. Methods: `get_mut(id)` (creates default),
`get(id)`, `most_trusted(agents, decay_rate)`, `meets_threshold(id, threshold, decay_rate)`.
