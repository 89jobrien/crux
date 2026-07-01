---
crate: crux-macros
type: proc-macro
description: "Proc macros for the crux agentic DSL"
version: "0.3.0"
edition: "2024"
dependencies: []
macros:
  - name: "crux::agent"
    target: "async fn"
    generates: "Agent trait impl + CruxCtx injection"
  - name: "crux::harness"
    target: struct
    generates: "Default + serde + to_profile()"
  - name: "crux::evolve"
    target: "async fn"
    generates: "Agent impl + is_evolution_agent()"
modules:
  - name: agent
    purpose: "agent macro expansion"
  - name: evolve
    purpose: "evolve macro expansion"
  - name: harness
    purpose: "harness macro expansion"
  - name: parse
    purpose: "Shared attribute parsing utilities"
---

# crux-macros

Proc macros for the crux agentic DSL.

## Macros

### `#[crux::agent]`

Transforms an async function into a traced, replayable agent. Injects a
`CruxCtx` binding (`x`), wraps the return type into `Crux<T>`, and
generates an `Agent` trait impl.

Options:
- `registry = "name"` — bind to a `TaskRegistry`
- `checkpoint_every_step` — checkpoint after every `x.step()` call
- `replay = "strict"|"lenient"` — replay mode (default: strict)

### `#[crux::harness]`

Marks a struct as a harness profile configuration. Generates `Default`,
`Serialize`/`Deserialize`, and a `to_profile()` method.

### `#[crux::evolve]`

Same as `#[crux::agent]` but semantically marks the function as part of
the harness evolution loop.
