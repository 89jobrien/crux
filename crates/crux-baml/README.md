# crux-baml

BAML-backed structured LLM handlers for `crux-script`. It converts model output into generated Rust
types for extraction, decomposition, and goal-to-pipeline planning.

## Architecture role

Hand-written wrappers in `extract.rs` and `planner.rs` adapt generated BAML clients to
`HandlerRegistry`. `crux-agentic` optionally calls `register_all_with_plugins`, passing external
handler descriptions into the planner prompt.

```rust
use crux_baml::register_all;
use crux_script::HandlerRegistry;

let mut registry = HandlerRegistry::new();
register_all(&mut registry);
```

## Handlers and API

- `llm::extract`: dispatches a named structured function with an object input. The dispatcher
  supports `ExtractEntities`, `Summarize`, `Classify`, `DescribeProject`, `AssessHealth`,
  `ClassifyProject`, `GenerateChangelog`, `SuggestRelated`, and `ClassifyCIFailure` from
  `baml_src/extract.baml`.
- `llm::decompose`: turns task text into structured subtasks.
- `llm::plan`: generates pipeline YAML from a goal, available handler list, and constraints.
- `register_extract_with`: injects a test or application extraction function.
- `generate_pipeline`: directly invokes the generated planning client and serializes its result.

## Generated-code boundary

`baml_src/*.baml` is source. `src/baml_client/` is generated output and must not be edited manually.
Regenerate from this crate directory:

```console
mise exec -- baml-cli generate
```

The `baml` dependency and `baml_src/generators.baml` generator version are both `0.221.0` and must
move together. Generated lint exceptions are scoped in `lib.rs`.

## Features and implementation status

The crate has no Cargo feature flags. Real handlers call configured BAML clients and therefore need
provider credentials; injected and generated mock paths support deterministic tests. Planner output
is serialized YAML, but callers should still parse/validate it with `crux-script` before execution.

## Development and testing

```console
mise exec -- baml-cli generate
cargo nextest run -p crux-baml
cargo clippy -p crux-baml --all-targets -- -D warnings
```

Live-provider tests require the workspace's secret-injection setup. Mock tests cover registration,
extraction dispatch, and planning without external calls.
