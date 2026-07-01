---
crate: crux-types
pattern: data-only
constraints:
  - "No tokio, no async, no LLM client deps"
  - "All types Serialize + Deserialize"
  - "RecoveryKind is serializable subset of Recovery<T>"
  - "crux-runtime re-exports everything"
modules:
  - name: crux_value
    types: ["Crux<T>"]
    deps: ["serde", "step", "id", "budget"]
  - name: step
    types: ["Step", "StepKind", "StepStatus"]
    deps: ["serde", "chrono"]
  - name: id
    types: ["CruxId", "TaskId"]
    deps: ["ulid", "serde"]
  - name: budget
    types: ["Budget"]
    deps: ["serde", "chrono"]
  - name: error
    types: ["CruxErr"]
    deps: ["serde", "thiserror"]
  - name: recovery
    types: ["Recovery<T>", "RecoveryKind"]
    deps: ["serde"]
  - name: emission
    types: ["Emission event types"]
    deps: ["serde"]
---

# Architecture: crux-types

Pure data crate. No behavior beyond serde serialization and basic
constructors. Designed as the leaf dependency that other crates and
external consumers can depend on without pulling async or LLM deps.

## Type Hierarchy

```
Crux<T>
 +-- steps: Vec<Step>
 |    +-- kind: StepKind
 |    +-- status: StepStatus
 |    +-- confidence: Option<f64>
 |    +-- children: Vec<Step>
 +-- result: Result<T, CruxErr>
 +-- id: CruxId
 +-- budget: Budget
```

## Design Constraints

- No `tokio`, no `async`, no LLM client deps
- `RecoveryKind` is the serializable subset of `Recovery<T>` (closure
  variants stay in crux-runtime)
- `crux-runtime` re-exports everything — moving types here is not a
  breaking change for consumers of `crux`
