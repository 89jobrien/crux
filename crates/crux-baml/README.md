---
crate: crux-baml
type: integration
description: "BAML-powered LLM handlers for crux-script pipelines"
version: "0.3.0"
edition: "2024"
dependencies:
  - baml
  - crux-script
handlers:
  - "llm::extract"
  - "llm::decompose"
  - "llm::plan"
generated_dirs:
  - baml_client/
---

# crux-baml

BAML-powered LLM handlers for crux-script pipelines. Provides structured
output extraction, task decomposition, and planning via BAML type-safe
LLM calls.

## Handlers

- **`llm::extract`** — structured data extraction from unstructured text
- **`llm::decompose`** — break complex tasks into subtasks
- **`llm::plan`** — generate execution plans from goals

## BAML Client

The `baml_client/` directory is generated code — do not edit manually.
Regenerate with:

```bash
mise exec -- baml-cli generate
```

The `baml` crate version in `Cargo.toml` must match `version` in
`generators.baml` exactly.

## Dependencies

Requires API keys for LLM providers. Inject via `dotenvx` or `sops-run`:

```bash
just sops-run crux dev cargo nextest run -p crux-baml
```
