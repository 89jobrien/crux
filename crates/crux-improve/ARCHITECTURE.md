---
crate: crux-improve
pattern: protocol-types
file_count: 1
concepts:
  - Comparison
  - ImprovementStrategy
  - Diff
  - Policy
dependencies:
  - crux-types
---

# Architecture: crux-improve

Single-file crate defining the improvement protocol vocabulary.

## Concept Model

```
Trace A (baseline)
  vs
Trace B (candidate)
  -> Comparison
  -> ImprovementStrategy
  -> Diff (what changed)
  -> Policy (accept/reject)
```

## Dependencies

Re-exports core trace types from `crux-types` so consumers of the
improvement protocol only need one dependency. No runtime or async
dependencies.
