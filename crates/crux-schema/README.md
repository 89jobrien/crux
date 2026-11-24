# crux-schema

Runtime-independent trace schema for Crux. The crate defines `Crux<T>`, the value that fuses a
result with its causal execution trace, along with the types that describe that trace: delegations
to child agents, the severity of a finished step, and per-step records used when aggregating a
final phase.

It has no async code and no runtime dependency, so tools that only read or render traces can use
it without pulling in the full `crux-runtime`. `crux-runtime` re-exports these types as
`types::crux_value`.

## What it provides

- `Crux<T>` holds the agent name, optional pipeline version, the `Result<T, CruxErr>` value, the
  flat list of `Step`s, child traces for delegations, and start and finish timestamps.
- Inspection methods: `value`, `into_value`, `causal_chain`, `delegations`, `rejected_branches`,
  `duration_ms`, `succeeded_count`, and `failed_count`.
- Rendering: `to_trace_json` produces a presentation-format JSON trace (hashes omitted, durations
  computed, children nested), and `to_mermaid` produces a `graph TD` flowchart colored by step
  status.
- `to_snapshot` converts a `Crux<T>` into a type-erased `Crux<serde_json::Value>` for
  checkpointing.
- `FinalPhase` orders workflow outcomes by severity (`Succeeded` < `Skipped` < `Aborted` <
  `Failed` < `Errored`), so `Ord` gives the aggregate outcome.
- `StepRecord` carries the alias, phase, and `continue_on_error` flag for each step during that
  aggregation.

## Feature flags

| Flag         | Default | Effect                                                               |
| ------------ | ------- | -------------------------------------------------------------------- |
| `test-utils` | no      | Compiles the `testing` module, which provides the `crux_ok` builder. |

The `testing` module is also compiled under `cfg(test)`.

## Example

```rust
use crux_schema::Crux;

fn summarize(trace: &Crux<String>) {
    if let Ok(text) = trace.value() {
        println!("{text}");
    }
    println!("{} steps, {} failed", trace.causal_chain().len(), trace.failed_count());
    println!("{}", trace.to_mermaid());
}
```

## Development

```console
cargo nextest run -p crux-schema
cargo nextest run -p crux-schema --features test-utils
cargo clippy -p crux-schema --all-targets -- -D warnings
```
