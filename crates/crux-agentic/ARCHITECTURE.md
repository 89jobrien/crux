---
crate: crux-agentic
pattern: handler-library
entry_point: "register_all()"
handler_domains:
  - name: llm
    purpose: "LLM interaction (chat, extract, decompose)"
  - name: container
    purpose: "Container lifecycle management"
  - name: harness
    purpose: "Harness orchestration"
  - name: ci
    purpose: "CI/CD pipeline handlers"
  - name: analysis
    purpose: "Code analysis"
  - name: review
    purpose: "Code review"
  - name: discover
    purpose: "Service/tool discovery"
  - name: planner
    purpose: "Planning/decomposition"
  - name: rx
    purpose: "Reactive step handlers"
  - name: sqlite
    purpose: "SQLite persistence"
  - name: triage
    purpose: "Issue triage"
adapters:
  - name: AutoApproveGate
    port: ApprovalGate
  - name: TerminalApprovalGate
    port: ApprovalGate
---

# Architecture: crux-agentic

Handler library organized by capability domain. Each module registers
handlers into a `HandlerRegistry` from crux-script.

## Registration

`register_all()` is the single entry point. It calls each module's
`register()` function and optionally includes stdlib and BAML handlers.

## Adapters

`adapters/` provides implementations of runtime ports:
- `AutoApproveGate` — approves all actions (for testing/trusted envs)
- `TerminalApprovalGate` — prompts user via terminal

## BAML Dependency

LLM structured output goes through `crux-baml`. The `baml_client/`
directory is generated code managed by `baml-cli`.
