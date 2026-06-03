---
title: Crux Workspace Architecture
source_document: crux_types_crate, crux_runtime_crate, crux_macros_crate, crux_remaining_crates
tags: [architecture, overview, workspace]
---

# Crux Workspace Architecture

Crux is an agentic DSL for Rust. Every step, delegation, speculation, and
failure is a first-class value ([[Crux<T>]]) that is inspectable, serializable,
and replayable. Rust edition 2024, MSRV 1.88.

## Crate Dependency Graph

```
crux (facade)
 +-- crux-macros (proc macros: agent, harness, evolve)
 +-- crux-runtime (core domain logic)
 |    +-- crux-types (wire-format, minimal deps)
 +-- crux-script (YAML pipeline scripting) [optional]

crux-agentic (step handlers + adapters)
 +-- crux-runtime
 +-- crux-script (handler registration)
 +-- crux-model (model ID parsing)

crux-planner (goal-to-pipeline generation)
 +-- crux-runtime

crux-plugin (subprocess plugin host)
 +-- crux-script

crux-model (standalone, no internal deps)
```

## Hexagonal Architecture

Ports (traits):
- [[Agent]] -- agentic work units
- [[Context]] -- runtime abstraction (DIP)
- [[RegistryBackend]] -- task persistence
- [[LlmProvider]] -- LLM completion
- [[ContainerClient]] -- container runtime
- [[ApprovalGate]] -- human-in-the-loop
- [[SafetyPolicy]] -- harness change validation
- [[Redactor]] -- output scrubbing
- [[AuditSink]] -- governance logging
- [[PipelineGenerator]] -- goal-to-pipeline
- [[PluginDiscovery]] -- plugin loading

Adapters:
- [[InMemoryBackend]], [[RedbBackend]] for [[RegistryBackend]]
- [[AnthropicAdapter]], [[OpenAiAdapter]], [[OllamaAdapter]] for [[LlmProvider]]
- [[DockerContainerClient]] for [[ContainerClient]]
- [[AutoApproveGate]], [[TerminalApprovalGate]] for [[ApprovalGate]]
- [[InMemoryAudit]] for [[AuditSink]]

## Key Design Patterns

1. **Trace-as-value**: [[Crux<T>]] fuses result with execution trace
2. **SOLID decomposition**: [[CruxCtx]] delegates to [[StepRecorder]],
   [[HookRegistry]], [[ReplayCache]], [[BudgetTracker]]
3. **Proc macro codegen**: [[#[crux::agent]]] generates Agent impl + wrapper
4. **Confidence routing**: [[CruxCtx]].route_on_confidence validates coverage
5. **Replay**: Steps matched by name + ordinal hash; lenient mode does
   forward scan
