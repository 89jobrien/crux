# crux-model

Canonical LLM model identifiers and provider-specific parsers. The crate preserves the provider's
raw identifier while deriving a stable key for routing, accounting, and logs.

## Architecture role

This is a pure parsing/data layer used by provider adapters. It does not make network calls.
`ProviderModelId` dispatches to strict parsers for known vendors or to a lenient fallback that keeps
unknown-but-usable identifiers representable.

## Usage

```rust
use crux_model::{ProviderModelId, Vendor};

let model = ProviderModelId::parse(Vendor::Anthropic, "claude-3-5-sonnet-20241022")?;
assert_eq!(model.vendor, Vendor::Anthropic);
println!("{}", model.canonical.as_key());
# Ok::<(), crux_model::ModelParseError>(())
```

## Key API

- `Vendor`: OpenAI, Anthropic, Google, Meta, Mistral, Cohere, Ollama, and Local.
- `ProviderModelId::parse`: validated provider-specific parsing.
- `ProviderModelId::parse_lenient`: always returns a provider reference using fallback parsing.
- `CanonicalModelId`: vendor, family, generation, and variant plus `as_key()`.
- `ProviderModelRef`: raw provider ID, canonical ID, and optional `ModelMetadata`.
- Provider parser modules: OpenAI, Anthropic, Google, Mistral, Ollama, and fallback.
- `ollama::enrich_from_api_entry`: extracts family, parameter size, quantization, and format.

## Features and status

There are no Cargo features and no generated files. Meta, Cohere, and Local are valid vendors but
currently use fallback normalization rather than dedicated parser modules. Canonicalization is a
compatibility boundary: changing it can alter routing, billing aggregation, and persisted keys.

## Development and testing

```console
cargo nextest run -p crux-model
cargo clippy -p crux-model --all-targets -- -D warnings
```

Unit and property tests cover real provider strings, malformed IDs, case-insensitive vendor parsing,
serde round trips, lenient fallback, and Ollama metadata.
