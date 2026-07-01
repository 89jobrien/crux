---
crate: crux-model
pattern: parser-pipeline
flow:
  - stage: parse
    input: "Raw string (e.g. gpt-4o-2024-08-06)"
    output: ProviderModelId
    module: parser
  - stage: detect
    input: ProviderModelId
    output: Vendor
    module: vendor
  - stage: normalize
    input: ProviderModelId
    output: CanonicalModelId
    module: canonical
  - stage: resolve
    input: CanonicalModelId
    output: ProviderModelRef
    module: provider_ref
---

# Architecture: crux-model

Model ID normalization pipeline:

## Flow

```
Raw string ("gpt-4o-2024-08-06")
  -> parser::ProviderModelId::parse()
  -> vendor detection (Vendor enum)
  -> canonical::CanonicalModelId (normalized form)
  -> provider_ref::ProviderModelRef (with metadata)
```

## Extension

Add new providers by extending the `Vendor` enum and adding parser
patterns in `parser/`.
