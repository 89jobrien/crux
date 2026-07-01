---
crate: crux-runtime
pattern: hexagonal
ports:
  - name: RegistryBackend
    module: registry
    purpose: "Task persistence"
  - name: Context
    module: context
    purpose: "DIP abstraction over CruxCtx"
  - name: SafetyPolicy
    module: safety
    purpose: "Diff approval logic"
  - name: ApprovalGate
    module: approval
    purpose: "Human-in-the-loop decisions"
  - name: GovernancePolicy
    module: governance
    purpose: "Composable policy enforcement"
  - name: AuditSink
    module: audit
    purpose: "Audit event recording"
  - name: TrustRegistry
    module: trust
    purpose: "Agent trust scoring"
adapters:
  - name: InMemoryBackend
    port: RegistryBackend
    feature: null
  - name: RedbBackend
    port: RegistryBackend
    feature: redb
collaborators:
  - name: HookRegistry
    file: "src/hooks.rs"
    purpose: "Lifecycle hook dispatch"
  - name: StepRecorder
    file: "src/recorder.rs"
    purpose: "Appends steps to trace"
  - name: ReplayCache
    file: "src/replay.rs"
    purpose: "Step output cache"
---

# Architecture: crux-runtime

## Hexagonal Design

The runtime follows ports-and-adapters architecture. Domain logic is
isolated from infrastructure through trait boundaries.

## CruxCtx Collaborators

`CruxCtx` delegates to independently testable collaborators:

```
CruxCtx
 +-- HookRegistry     (hooks.rs)     — lifecycle hook dispatch
 +-- StepRecorder     (recorder.rs)  — appends steps to trace
 +-- ReplayCache      (replay.rs)    — step output cache
```

## Replay

Steps are matched by name + ordinal hash (`hash_step_identity`). Strict
mode fails on mismatch. Lenient mode does a forward name scan — the scan
is the designed recovery path, not a fallback.

## Prelude

`prelude` module re-exports the most commonly used types from both
`crux-runtime` and `crux-domain` for ergonomic imports.
