---
crate: crux-agentic
type: handlers
description: "Built-in step handlers for crux-script pipelines"
version: "0.3.0"
edition: "2024"
dependencies:
  - crux-script
  - crux-runtime
  - crux-stdlib
  - crux-baml (optional)
modules:
  - name: handlers
    purpose: "Handler registration entry point"
  - name: llm
    purpose: "LLM interaction handlers"
  - name: llm_step
    purpose: "LLM step type definitions"
  - name: container
    purpose: "Container management handlers"
  - name: harness
    purpose: "Harness orchestration handlers"
  - name: ci
    purpose: "CI/CD pipeline handlers"
  - name: discover
    purpose: "Service/tool discovery"
  - name: analysis
    purpose: "Code analysis handlers"
  - name: review
    purpose: "Code review handlers"
  - name: planner
    purpose: "Planning/decomposition handlers"
  - name: provider
    purpose: "LLM provider abstraction"
  - name: rx
    purpose: "Reactive step handlers"
  - name: sqlite
    purpose: "SQLite-backed persistence"
  - name: triage
    purpose: "Issue triage handlers"
  - name: adapters
    purpose: "AutoApproveGate, TerminalApprovalGate"
---

# crux-agentic

Built-in step handlers for crux-script pipelines. Agentic capabilities:
shell execution, filesystem ops, git integration, LLM calls, container
management, harness orchestration, and CI/CD steps.

## Usage

```rust
use crux_agentic::register_all;
use crux_script::HandlerRegistry;

let mut registry = HandlerRegistry::new();
register_all(&mut registry);
```

## BAML Integration

LLM structured output handlers (`llm::extract`, `llm::decompose`) require
the `baml` feature flag. The `baml_client/` directory is generated — run
`mise exec -- baml-cli generate` from this crate's directory after cloning.
