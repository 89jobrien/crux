---
crate: crux-types
role: wire-types
constraints:
  - "No tokio, no async, no LLM client dependencies"
  - "All types must implement Serialize + Deserialize"
  - "Moving types here from crux-runtime is not breaking"
  - "RecoveryKind is serializable subset — closures stay in runtime"
howto:
  - task: "Add a new type"
    steps:
      - "Create type in appropriate module"
      - "Derive Serialize, Deserialize, Debug, Clone"
      - "Add #[cfg(test)] roundtrip test"
      - "Re-export from crux-runtime if consumers need it"
pitfalls:
  - "Do not add deps that pull in tokio or async runtimes"
  - "Keep CruxId generation deterministic for replay"
---

# Agents: crux-types

## For AI Agents Working With This Crate

Wire-format types crate. **Minimal dependencies** is the primary
design constraint.

### Rules

- No `tokio`, no `async`, no LLM client dependencies
- All types must implement `Serialize + Deserialize`
- Moving a type here from `crux-runtime` is not a breaking change
  (runtime re-exports everything)
- `RecoveryKind` is the serializable subset — closure variants stay
  in `crux-runtime`
