---
crate: crux-model
type: parser
description: "Canonical model ID types and provider-specific parsers"
version: "0.3.0"
edition: "2024"
dependencies:
  - serde
key_types:
  - CanonicalModelId
  - ProviderModelId
  - ProviderModelRef
  - ModelMetadata
  - Vendor
  - ModelParseError
modules:
  - name: canonical
    purpose: "CanonicalModelId and normalization"
  - name: parser
    purpose: "Provider-specific model string parsing"
  - name: provider_ref
    purpose: "ProviderModelRef with metadata"
  - name: vendor
    purpose: "Vendor enum and detection"
  - name: error
    purpose: "Error types"
---

# crux-model

Canonical model ID types and provider-specific parsers for crux.
Normalizes LLM model identifiers across providers (OpenAI, Anthropic,
Google, etc.) into a canonical form for consistent routing and billing.

## Key Types

- **`CanonicalModelId`** — normalized model identifier
- **`ProviderModelId`** — raw provider-specific model string
- **`ProviderModelRef`** — resolved reference with metadata
- **`ModelMetadata`** — capabilities and pricing info
- **`Vendor`** — provider enum (OpenAI, Anthropic, Google, etc.)
- **`ModelParseError`** — parsing failure type
