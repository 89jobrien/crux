# crux

Agentic workflows as YAML pipelines, backed by a typed Rust runtime.

- **Write pipelines in YAML.** Define steps, fan-out, piping, and
  budgets in `.crux` files. The runtime handles execution, tracing,
  and error recovery.
- **Every step is traced.** Each step lands in a typed `Crux<T>` value
  you can inspect, serialize, or replay after a crash.
- **Rust when you need it.** Drop into `#[crux::agent]` for custom
  logic, typed delegation, and confidence-based routing -- same trace,
  same runtime.

## Quick example

A pipeline as a `.crux` file:

```yaml
pipeline: summarize
budget: { calls: 2 }

steps:
    - step: count_words
      handler: shell::capture
      args:
          cmd: "wc -w < input.txt"

    - step: log_result
      handler: ctrl::log
```

The same thing in Rust:

```rust
use crux::prelude::*;

#[crux::agent]
async fn summarize(input: String) -> Crux<String> {
    let count: usize = x.step("count_words", || async {
        Ok(input.split_whitespace().count())
    }).await?;

    x.step("format", || async move {
        Ok(format!("{count} words"))
    }).await
}
```

Either way, the returned `Crux<T>` is:

- **Inspectable** -- `crux.causal_chain()`, `crux.delegations()`
- **Serializable** -- `serde_json::to_string(&crux)`
- **Replayable** -- `Crux::replay_from(snapshot)` resumes after a crash

## Installation

```toml
[dependencies]
crux = "0.2"
```

Requires Rust 1.88+ (edition 2024).

## Crates

| Crate                                 | Description                                             |
| ------------------------------------- | ------------------------------------------------------- |
| [`crux`](crates/crux)                 | Facade -- re-exports runtime + macros                   |
| [`crux-runtime`](crates/crux-runtime) | Core types, traits, and runtime                         |
| [`crux-types`](crates/crux-types)     | Wire-format types (`Crux<T>`, `Step`, `Budget`)         |
| [`crux-derive`](crates/crux-macros)   | `#[crux::agent]`, `#[crux::harness]`, `#[crux::evolve]` |
| [`crux-agentic`](crates/crux-agentic) | Step handlers: shell, fs, git, llm, container           |
| [`crux-script`](crates/crux-script)   | YAML pipeline scripting                                 |
| [`crux-model`](crates/crux-model)     | Model ID types and provider parsers                     |
| [`crux-plugin`](crates/crux-plugin)   | Subprocess plugin host                                  |
| [`crux-planner`](crates/crux-planner) | Metrics-driven harness evolution                        |
| [`crux-domain`](crates/crux-domain)   | Pure domain types -- no async, no LLM deps              |
| [`crux-baml`](crates/crux-baml)       | BAML-powered LLM handlers (extract, decompose, plan)    |
| [`crux-stdlib`](crates/crux-stdlib)   | Standard library handlers (fs, git, json, text, ctrl)   |
| [`crux-task`](crates/crux-task)       | Project task management                                 |
| [`crux-improve`](crates/crux-improve) | Improvement protocol: strategies, diffs, comparisons    |

## Feature flags

| Flag            | Default | Description                        |
| --------------- | ------- | ---------------------------------- |
| `tokio-runtime` | yes     | Async support via tokio + futures  |
| `redb`          | no      | Persistent `TaskRegistry` via redb |
| `tracing`       | no      | Instrument with `tracing` spans    |
| `baml`          | no      | BAML-backed LLM extraction         |

## Documentation

- [Tutorial](docs/walkthrough/README.md) -- chapter-by-chapter walkthrough
- [Handlers and capabilities](docs/crux-capabilities.md) -- pipeline handlers, support matrix
- [Syntax reference](docs/crux-syntax-reference.md) -- pipeline YAML syntax
- [Plugin system](docs/crux-plugins.md) -- subprocess plugin host

## License

MIT -- see [LICENSE](LICENSE).
