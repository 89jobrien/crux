---
crate: crux-agentic
test_strategy: mixed
inline_test_modules: 8
dedicated_test_files: 26
requires_api_keys: true
test_areas:
  - module: handlers
    coverage: "Handler registration completeness"
  - module: llm
    coverage: "LLM handler dispatch (mocked and live)"
  - module: container
    coverage: "Container lifecycle handlers"
  - module: harness
    coverage: "Harness orchestration"
  - module: ci
    coverage: "CI step handlers"
  - module: adapters
    coverage: "ApprovalGate implementations"
  - module: analysis
    coverage: "Code analysis handlers"
  - module: review
    coverage: "Code review handlers"
commands:
  default: "cargo nextest run -p crux-agentic"
  baml: "cargo nextest run -p crux-agentic --features baml"
  with_keys: "just sops-run crux dev cargo nextest run --features baml -p crux-agentic"
---

# Testing: crux-agentic

## Test Strategy

8 inline test modules + 26 dedicated test files. Largest test surface in
the workspace.

## Running

```bash
cargo nextest run -p crux-agentic                  # unit tests only
cargo nextest run -p crux-agentic --features baml   # include BAML tests
```

## BAML / LLM Tests

Tests that call live LLM APIs require API keys:

```bash
just sops-run crux dev cargo nextest run --features baml -p crux-agentic
```
