# crux

An agentic DSL for Rust -- inspectable, serializable, replayable agent orchestration.

`cruxx` is not a standalone language. It's a set of macros, traits, and types that make agentic
control flow explicit in the Rust type system. 

## Quick example

```rust
use cruxx::prelude::*;

#[cruxx::agent]
async fn review_pr(pr: PullRequest) -> Crux<ReviewReport> {
    // Fan out: fetch diff and CI results in parallel
    let (diff, ci) = x.join_all([
        x.step("fetch_diff", || git::diff(&pr.base, &pr.head)),
        x.step("fetch_ci",   || ci::latest_run(&pr.repo, &pr.head)),
    ]).await?;

    // Delegate deep analysis to a specialist; escalate if confidence is low
    let analysis = x.delegate::<SecurityAnalysisAgent>("security", &diff)
        .with_budget(Budget::tokens(8000))
        .on_low_confidence(0.75, escalate_to_human)
        .on_step_failure(Recovery::Retry(2))
        .await?;

    // Race three review styles; keep whichever scores highest
    let review = x.speculate("style", [
        ("strict",  || apply_strict_style(&analysis, &ci)),
        ("lenient", || apply_lenient_style(&analysis, &ci)),
        ("summary", || apply_summary_style(&analysis, &ci)),
    ]).pick_best_by(|r| r.confidence).await?;

    x.step("emit", || build_report(pr, analysis, review)).await
}
```

Every `x.step`, `x.delegate`, `x.speculate` call is recorded in the `Crux<T>` value
the function returns. That value is:

- **Inspectable**: `cruxx.causal_chain()`, `cruxx.delegations()`, `cruxx.rejected_branches()`
- **Serializable**: `serde_json::to_string(&cruxx)` just works
- **Replayable**: `Crux::replay_from(snapshot)` resumes after a crash
- **Composable**: `cruxx_a | cruxx_b`, `Crux::join_all([...])`

## Crates

| Crate                                   | Description                                              |
| --------------------------------------- | -------------------------------------------------------- |
| [`cruxx`](crates/cruxx)                 | Facade crate, re-exports `cruxx-core` + `cruxx-macros`                       |
| [`cruxx-core`](crates/cruxx-core)       | Core types, traits, and runtime                                              |
| [`cruxx-types`](crates/cruxx-types)     | Serializable wire-format types (`Crux<T>`, `Step`, `Budget`, `RecoveryKind`) |
| [`cruxx-macros`](crates/cruxx-macros)   | `#[cruxx::agent]`, `#[cruxx::harness]`, `#[cruxx::evolve]` macros            |
| [`cruxx-script`](crates/cruxx-script)   | YAML-driven pipeline scripting                                      |
| [`cruxx-agentic`](crates/cruxx-agentic) | Step handlers: shell, fs, git, json, llm, container, harness        |
| [`cruxx-model`](crates/cruxx-model)     | Canonical model ID types and provider-specific parsers              |
| [`cruxx-plugin`](crates/cruxx-plugin)   | Subprocess plugin host for pipelines                                |
| [`cruxx-planner`](crates/cruxx-planner) | `EvolutionPlanner`: metrics-driven harness profile evolution        |

## Features

Enable via `cruxx`:

| Feature         | Default | Description                                                                |
| --------------- | ------- | -------------------------------------------------------------------------- |
| `tokio-runtime` | yes     | Async runtime support via tokio + futures                                  |
| `redb`          | no      | Persistent `TaskRegistry` backend via redb (pure-Rust)                     |
| `tracing`       | no      | Instrument with `tracing` spans                                            |
| `baml`          | no      | BAML-backed LLM extraction (`llm::extract`, `llm::decompose`, `llm::plan`) |

## Core concepts

**`Crux<T>`**: the execution trace. Every step, delegation, speculation, and failure is a
first-class value you can inspect, serialize, and replay.

**`CruxCtx`**: the runtime context threaded through agent execution. Provides `step()`,
`delegate()`, `speculate()`, `pipe()`, `join_all()`, `route_on_confidence()`.

**`Agent` trait**: the single-method interface all agents implement. The `#[cruxx::agent]` macro
generates this for you.

**`TaskRegistry<B>`**: typed task management with submit, checkpoint, replay, and status
transitions. Pluggable backend (`InMemoryBackend`, `RedbBackend`).

**Lifecycle hooks**: `on_low_confidence`, `on_step_failure`, `on_budget_exceeded` with recovery
actions (skip, retry, escalate, substitute).

**Replay**: strict or lenient mode. Strict rejects hash mismatches; lenient skips removed steps
and returns cache misses for changed ones.

**`HarnessProfile`**: resource specification for a container or process harness (image, env,
limits). Paired with `ResourceHints` for advisory scheduling metadata and `HarnessDiff` to
describe incremental profile changes.

**`SafetyPolicy` trait**: port for user-defined approval logic. Receives a proposed
`HarnessDiff` and returns `Approved`, `Rejected`, or `RequiresApproval`. Two adapters ship in
`cruxx-agentic`: `AutoApproveGate` (always approves) and `TerminalApprovalGate` (interactive
stdin prompt).

**`EvolutionPlanner`** (`cruxx-planner`): drives deterministic, metrics-based profile
evolution. Accepts `RunMetrics` and emits a `HarnessDiff` describing resource adjustments.
`EvolutionOutcome` records the result of applying a diff.

## Orchestrator patterns

The `harness::evolve` and `harness::canary` pipeline handlers expose container lifecycle
management as first-class pipeline steps.

```yaml
steps:
  - name: evolve_profile
    handler: harness::evolve
    args:
      profile: base
      metrics_from: run_metrics

  - name: canary
    handler: harness::canary
    args:
      image: myapp:next
      traffic_percent: 10
```

Use `#[cruxx::harness]` to annotate a struct as a managed harness, and `#[cruxx::evolve]` to
mark an `async fn` as an evolution entry point (injects `EvolutionPlanner` + `CruxCtx`):

```rust
#[cruxx::harness]
struct ApiServer { image: String, replicas: u32 }

#[cruxx::evolve]
async fn scale_on_p99(metrics: RunMetrics) -> Crux<EvolutionOutcome> {
    let diff = planner.suggest(&metrics).await?;
    x.step("apply", || harness.apply_diff(&diff)).await
}
```

The `on_approval_required` lifecycle hook fires when `SafetyPolicy` returns `RequiresApproval`,
giving agents an opportunity to pause, log, or escalate before a diff is applied.

## Installation

```toml
[dependencies]
cruxx = "0.1"

# With persistent storage (redb, pure-Rust):
# cruxx = { version = "0.1", features = ["redb"] }
```

Requires Rust 1.85+ (edition 2024).

## Running pipelines

`cruxx run` executes YAML pipelines using the built-in handler registry. Build it with the `baml`
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
cruxx run examples/extract_summary.crux examples/input_summary.json
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
cruxx run examples/extract_entities.crux examples/input_entities.json
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
| `llm::invoke`       | `prompt`, `provider`, `model` | Raw LLM completion (OpenAI/Anthropic/Ollama)      |
| `container::run`    | `image`, `env`, `limits`      | Start a container from a `HarnessProfile`         |
| `container::wait`   | `timeout_ms`                  | Block until container exits, emit exit code/logs  |
| `harness::evolve`   | `profile`, `metrics_from`     | Run `EvolutionPlanner` and apply resulting diff   |
| `harness::canary`   | `image`, `traffic_percent`    | Deploy canary alongside current harness           |

**Behind `--features baml`:**

| Handler          | Key args            | Description                               |
| ---------------- | ------------------- | ----------------------------------------- |
| `llm::extract`   | `function`, `input` | BAML structured extraction                |
| `llm::decompose` | `spec`              | Spec decomposition into task list         |
| `llm::plan`      | `goal`              | Pipeline generation from natural language |

See [docs/crux-capabilities.md](docs/crux-capabilities.md) for the full support
matrix including combinators and known gaps.

## Examples

### Rust agents

```bash
cargo run --example basic_agent
```

See [`examples/`](examples/) for pipeline `.crux` files and input fixtures.

## Documentation

See the [tutorial](docs/walkthrough/README.md) for a chapter-by-chapter walkthrough.

## License

MIT -- see [LICENSE](LICENSE).
