---
crate: crux-baml
test_strategy: integration
inline_test_modules: 1
dedicated_test_files: 4
requires_api_keys: true
prerequisites:
  - "baml_client/ generated via: mise exec -- baml-cli generate"
  - "API keys injected via sops-run or dotenvx"
commands:
  default: "just sops-run crux dev cargo nextest run -p crux-baml"
  single: "just sops-run crux dev cargo nextest run -p crux-baml --test llm_extract"
---

# Testing: crux-baml

## Test Strategy

1 inline test module + 4 dedicated test files. Tests cover BAML handler
integration with live LLM providers.

## Running

All tests require API keys:

```bash
just sops-run crux dev cargo nextest run -p crux-baml
just sops-run crux dev cargo nextest run -p crux-baml --test llm_extract
```

## Prerequisites

- `baml_client/` must be generated: `mise exec -- baml-cli generate`
- API keys injected via `sops-run` or `dotenvx`
