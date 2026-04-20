# cruxx

An agentic DSL for Rust -- inspectable, serializable, replayable agent execution.

`cruxx` is not a standalone language. It's a set of macros, traits, and types that make agentic
control flow explicit in the Rust type system. If you've written agents with `tokio` + `tracing`
\+ a hand-rolled task queue, `cruxx` is what happens when you bake those patterns into the language
itself.

## Quick example

```rust
use cruxx::prelude::*;

#[cruxx::agent]
async fn plan_trip(goal: String) -> Crux<Itinerary> {
    let research = x.step("research", || search_web(&goal)).await?;

    let draft = x.delegate::<DraftAgent>("draft", &research)
        .with_budget(Budget::tokens(4000))
        .on_low_confidence(0.7, escalate_to_human)
        .await?;

    x.speculate("finalize", [
        ("cheap", || finalize_cheap(&draft)),
        ("fast",  || finalize_fast(&draft)),
        ("safe",  || finalize_safe(&draft)),
    ]).pick_best_by(|r| r.confidence).await
}
```

Every `x.step`, `x.delegate`, `x.speculate` call is recorded in the `Crux<T>` value
the function returns. That value is:

- **Inspectable** -- `cruxx.causal_chain()`, `cruxx.delegations()`, `cruxx.rejected_branches()`
- **Serializable** -- `serde_json::to_string(&cruxx)` just works
- **Replayable** -- `Crux::replay_from(snapshot)` resumes after a crash
- **Composable** -- `cruxx_a | cruxx_b`, `Crux::join_all([...])`

## Crates

| Crate                                   | Description                                              |
| --------------------------------------- | -------------------------------------------------------- |
| [`cruxx`](crates/cruxx)                 | Facade crate -- re-exports `cruxx-core` + `cruxx-macros` |
| [`cruxx-core`](crates/cruxx-core)       | Core types, traits, and runtime                          |
| [`cruxx-macros`](crates/cruxx-macros)   | `#[cruxx::agent]` proc macro                             |
| [`cruxx-script`](crates/cruxx-script)   | YAML-driven pipeline scripting                           |
| [`cruxx-agentic`](crates/cruxx-agentic) | Built-in step handlers (shell, fs, git, json, llm)       |
| [`cruxx-model`](crates/cruxx-model)     | Canonical model ID types and provider-specific parsers   |
| [`cruxx-plugin`](crates/cruxx-plugin)   | Subprocess plugin host for pipelines                     |
| [`cruxx-planner`](crates/cruxx-planner) | Goal-to-pipeline planner for cruxx-script                |

## Features

Enable via `cruxx`:

| Feature         | Default | Description                                                                |
| --------------- | ------- | -------------------------------------------------------------------------- |
| `tokio-runtime` | yes     | Async runtime support via tokio + futures                                  |
| `redb`          | no      | Persistent `TaskRegistry` backend via redb (pure-Rust)                     |
| `tracing`       | no      | Instrument with `tracing` spans                                            |
| `baml`          | no      | BAML-backed LLM extraction (`llm::extract`, `llm::decompose`, `llm::plan`) |

## Core concepts

**`Crux<T>`** -- the execution trace. Every step, delegation, speculation, and failure is a
first-class value you can inspect, serialize, and replay.

**`CruxCtx`** -- the runtime context threaded through agent execution. Provides `step()`,
`delegate()`, `speculate()`, `pipe()`, `join_all()`, `route_on_confidence()`.

**`Agent` trait** -- the single-method interface all agents implement. The `#[cruxx::agent]` macro
generates this for you.

**`TaskRegistry<B>`** -- typed task management with submit, checkpoint, replay, and status
transitions. Pluggable backend (`InMemoryBackend`, `RedbBackend`).

**Lifecycle hooks** -- `on_low_confidence`, `on_step_failure`, `on_budget_exceeded` with recovery
actions (skip, retry, escalate, substitute).

**Replay** -- strict or lenient mode. Strict rejects hash mismatches; lenient skips removed steps
and returns cache misses for changed ones.

## Installation

```toml
[dependencies]
cruxx = "0.1"

# With persistent storage (redb, pure-Rust):
# cruxx = { version = "0.1", features = ["redb"] }
```

Requires Rust 1.85+ (edition 2024).

## Running pipelines

`cruxx-run` executes YAML pipelines using the built-in handler registry. Build it with the `baml`
feature to enable LLM extraction:

```bash
cargo build -p cruxx-agentic --features baml --bin cruxx-run
```

Set your API key — BAML picks it up automatically:

```bash
export ANTHROPIC_API_KEY=sk-ant-...   # Claude (default BAML client)
# or
export OPENAI_API_KEY=sk-...          # OpenAI
```

**Summarize text:**

```bash
cruxx-run examples/extract_summary.cruxx examples/input_summary.json
```

```
Pipeline: extract_summary
Status:   OK
Duration: 1823.4ms
Steps:    2

Trace:
   1. [  OK] summarize (1821ms)
   2. [  OK] log_output (1ms)

Output:
{
  "summary": "Crux is an agentic DSL for Rust that makes control flow explicit in the type
system via Crux<T> values.",
  "key_points": [
    "Every execution unit is a first-class Crux<T> value",
    "CruxCtx provides step(), delegate(), speculate(), pipe(), join_all()",
    "TaskRegistry supports InMemoryBackend and RedbBackend"
  ],
  "word_count": 89
}
```

**Extract named entities:**

```bash
cruxx-run examples/extract_entities.cruxx examples/input_entities.json
```

```
Pipeline: extract_entities
Status:   OK
Duration: 1540.2ms
Steps:    2

Trace:
   1. [  OK] extract (1538ms)
   2. [  OK] log_output (1ms)

Output:
{
  "entities": [
    { "name": "Crux",        "entity_type": "Software",   "description": "Agentic DSL for Rust" },
    { "name": "CruxCtx",     "entity_type": "Component",  "description": "Runtime context" },
    { "name": "RedbBackend", "entity_type": "Component",  "description": "Persistent KV adapter" }
  ]
}
```

### Available handlers

**Always available:**

| Handler             | Key args                      | Description                                  |
| ------------------- | ----------------------------- | -------------------------------------------- |
| `shell::exec`       | `cmd`                         | Run shell command, ignore exit code          |
| `shell::capture`    | `cmd`                         | Run shell command, fail on non-zero exit     |
| `fs::read`          | `path`                        | Read a file to string                        |
| `fs::write`         | `path`, `content`             | Write a string to a file                     |
| `fs::glob`          | `pattern`                     | Glob pattern match                           |
| `fs::exists`        | `path`                        | Check path existence                         |
| `git::staged_files` | —                             | `git diff --cached --name-only`              |
| `git::diff`         | `revision`                    | `git diff [revision]`                        |
| `git::log`          | `count`                       | `git log -N --format=%H\t%s`                 |
| `git::status`       | —                             | `git status --porcelain`                     |
| `json::pick`        | `fields`                      | Extract named fields from input object       |
| `json::merge`       | `with`                        | Merge static object into input               |
| `json::jq`          | `expr`                        | Dot-path traversal (e.g. `".foo.bar"`)       |
| `ctrl::noop`        | —                             | Pass input through unchanged                 |
| `ctrl::log`         | —                             | Log to stderr and pass through               |
| `ctrl::assert`      | `condition`                   | Assert condition is truthy or fail           |
| `llm::invoke`       | `prompt`, `provider`, `model` | Raw LLM completion (OpenAI/Anthropic/Ollama) |

**Behind `--features baml`:**

| Handler          | Key args            | Description                               |
| ---------------- | ------------------- | ----------------------------------------- |
| `llm::extract`   | `function`, `input` | BAML structured extraction                |
| `llm::decompose` | `spec`              | Spec decomposition into task list         |
| `llm::plan`      | `goal`              | Pipeline generation from natural language |

See [docs/cruxx-capabilities.md](docs/cruxx-capabilities.md) for the full support
matrix including combinators and known gaps.

## Examples

### Rust agents

```bash
cargo run --example basic_agent
```

See [`examples/`](examples/) for pipeline `.cruxx` files and input fixtures.

## Documentation

See the [tutorial](docs/walkthrough/README.md) for a chapter-by-chapter walkthrough.

## License

MIT -- see [LICENSE](LICENSE).
