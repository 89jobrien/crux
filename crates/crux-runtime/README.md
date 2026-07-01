---
crate: crux-runtime
type: runtime
description: "Core types, traits, and runtime for the crux agentic DSL"
version: "0.3.0"
edition: "2024"
dependencies:
  - crux-types
  - crux-domain
  - slashcrux
key_types:
  - CruxCtx
  - Agent
  - TaskRegistry
  - Recovery
  - Budget
modules:
  - name: agent
    purpose: "Agent trait definition"
  - name: ctx
    purpose: "CruxCtx and combinators"
  - name: delegation
    purpose: "DelegationBuilder with per-call budgets"
  - name: speculation
    purpose: "SpeculationBuilder with racing strategies"
  - name: hooks
    purpose: "HookRegistry for lifecycle dispatch"
  - name: recorder
    purpose: "StepRecorder for trace append"
  - name: replay
    purpose: "ReplayCache with strict/lenient modes"
  - name: registry
    purpose: "TaskRegistry with RegistryBackend port"
  - name: safety
    purpose: "SafetyPolicy trait"
  - name: approval
    purpose: "ApprovalGate trait"
  - name: governance
    purpose: "GovernancePolicy composable policies"
  - name: trust
    purpose: "TrustRegistry and TrustScore"
  - name: audit
    purpose: "AuditSink and AuditEntry"
---

# crux-runtime

Core runtime for the crux agentic DSL. Contains all domain logic: types,
traits, context, registry, replay, hooks, and combinators.

## Key Types

- **`CruxCtx`** — runtime context: `step()`, `delegate()`, `speculate()`,
  `pipe()`, `join_all()`, `route_on_confidence()`
- **`Agent`** — trait: `name()`, `run(ctx, input)`, `budget()`, lifecycle hooks
- **`TaskRegistry<B>`** — submit/get/update with CAS, pluggable backend
- **`Recovery<T>`** — hook return: Continue, Skip, Retry, Escalate, Substitute
- **`Budget`** — token/step/time limits, scoped per delegation

## Architecture

Hexagonal / ports-and-adapters. `RegistryBackend` is the persistence port
with `InMemoryBackend` (default) and `RedbBackend` (behind `redb` feature).
`Context` trait is the DIP abstraction over `CruxCtx` for testability.
