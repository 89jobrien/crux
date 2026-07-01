---
crate: crux
type: facade
description: "Facade crate — re-exports crux-runtime and crux-macros"
version: "0.3.0"
edition: "2024"
dependencies:
  - crux-runtime
  - crux-macros
  - crux-script (optional)
features:
  - name: tokio-runtime
    default: true
    effect: "Async support via tokio"
  - name: script
    default: false
    effect: "Re-exports crux-script as crux::script"
  - name: redb
    default: false
    effect: "Persistent RedbBackend for TaskRegistry"
  - name: tracing
    default: false
    effect: "Instrument with tracing spans"
---

# crux

Facade crate for the crux agentic DSL. Re-exports `crux-runtime` types and
`crux-macros` proc macros through a single dependency.

## Usage

```toml
[dependencies]
crux = { path = "../crux" }
```

```rust
use crux::prelude::*;

#[crux::agent]
async fn my_agent(input: String) -> Result<String, CruxErr> {
    let result = x.step("process", || Ok(input.to_uppercase())).await;
    Ok(result)
}
```

## What This Crate Provides

- `#[crux::agent]`, `#[crux::harness]`, `#[crux::evolve]` proc macros
- All runtime types via `crux::prelude::*`
- Integration tests covering macro expansion, combinators, delegation,
  speculation, and task registry
