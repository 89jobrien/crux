---
name: using-crux
description: >
  Work with the Crux Rust workspace, runtime, wire types, macros, CLI, pipeline
  engine, handlers, task store, or downstream Crux dependencies.
---

# Using Crux

Crux is an agentic Rust DSL with typed agents and YAML-syntax `.crux` pipelines.
Executions produce an inspectable, serializable, replayable `Crux<T>` trace.

- Rust edition: 2024
- MSRV: 1.89.0
- Workspace version: 0.3.1
- License: MIT
- Repository: `https://github.com/89jobrien/crux`

## Workspace layout

All packages are under `crates/` except `xtask`.

| Directory | Package | Role |
| --- | --- | --- |
| `crux` | `crux` | Facade re-exporting runtime and macros |
| `crux-runtime` | `crux-runtime` | Context, agents, combinators, replay, hooks, registry, governance |
| `crux-types` | `crux-types` | Wire types: traces, steps, budgets, errors, tasks |
| `crux-macros` | `crux-derive` | `agent`, `harness`, `evolve` proc macros |
| `crux-domain` | `crux-domain` | Actions, planners, events, pipeline vocabulary |
| `crux-script` | `crux-script` | `.crux` schema, validation, expressions, execution |
| `crux-stdlib` | `crux-stdlib` | Shell/fs/git/JSON/text/control handlers |
| `crux-agentic` | `crux-agentic` | LLM, container, analysis, CI, review, triage, SQLite, task handlers |
| `crux-baml` | `crux-baml` | Optional BAML-backed handlers |
| `crux-cli` | `crux-cli` | Builds the `crux` binary |
| `crux-task` | `crux-task` | Project task management |
| `crux-plugin` | `crux-plugin` | Subprocess plugin protocol and host |
| `crux-planner` | `crux-planner` | Harness evolution planner |
| `crux-model` | `crux-model` | Provider/model identifiers |
| `crux-improve` | `crux-improve` | Improvement-analysis library |

The facade package is `crux`, not `cruxx`. The proc-macro directory remains
`crux-macros`, but its package is `crux-derive`.

## Features

- `crux`: default `tokio-runtime`; optional `redb`, `tracing`, `script`.
- `crux-runtime`: default `tokio-runtime`; optional `redb`, `tracing`.
- `crux-agentic`: optional `baml`, `docker`.
- `crux-cli`: optional `baml`.

## Build and test

```bash
just check
just build
just fmt
just lint
just test             # cargo nextest run
just ci
```

Always use `cargo nextest run`, not `cargo test`.

```bash
cargo nextest run -p crux-runtime
cargo nextest run -p crux --test agent_macro
cargo nextest run --features redb
```

BAML tests are credentialed; do not run them without explicit credentials.
Never edit generated `crates/crux-baml/src/baml_client/` files.

## Dependencies

```toml
[dependencies]
crux = { git = "https://github.com/89jobrien/crux", rev = "<sha>" }
# wire types only
crux-types = { git = "https://github.com/89jobrien/crux", rev = "<sha>" }
# plugin protocol
crux-plugin = { git = "https://github.com/89jobrien/crux", rev = "<sha>" }
```

Use `use crux::prelude::*;` with the facade.

## References

- `references/types-and-traits.md`
- `references/architecture.md`
- `references/capabilities.md`
- `../planning-with-crux/references/handler-catalog.md`
