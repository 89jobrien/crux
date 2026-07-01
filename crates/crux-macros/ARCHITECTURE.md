---
crate: crux-macros
pattern: proc-macro
pipeline: "attr + item -> parse -> expand -> TokenStream"
modules:
  - name: agent
    input: "async fn"
    output: "Inner fn + wrapper + Agent impl"
  - name: harness
    input: struct
    output: "Default + serde + to_profile()"
  - name: evolve
    input: "async fn"
    output: "Same as agent + is_evolution_agent()"
  - name: parse
    input: "Attribute tokens"
    output: "Parsed option structs"
generated_bindings:
  - "crux_runtime::*"
---

# Architecture: crux-macros

Proc-macro crate. Each macro has its own module with an `expand` function
that takes `proc_macro2::TokenStream` and returns `syn::Result<TokenStream>`.

## Expansion Pipeline

```
#[crux::agent] attr + item
  -> parse::parse_agent_attrs()   — extract options
  -> agent::expand()              — generate wrapper + Agent impl
  -> TokenStream output
```

## Generated Code Shape

### `#[crux::agent]`

1. Renames original fn to `__inner`
2. Creates wrapper that constructs `CruxCtx` (`x`)
3. Calls inner fn, then `finalize()`
4. Generates `FooAgent` struct implementing `Agent` trait

### `#[crux::harness]`

Derives `Default`, `Serialize`, `Deserialize` and adds a `to_profile()`
method that maps struct fields to `HarnessProfile`.
