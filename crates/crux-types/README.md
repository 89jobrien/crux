# crux-types

Serializable wire-format and accounting types for Crux traces. This is the lightweight dependency
for storing, transporting, inspecting, or routing traces without the full runtime.

## Architecture role

`crux-types` owns stable data crossing process and workspace boundaries. `crux-runtime`
re-exports these types and adds behavior around them. The base build has no async runtime;
optional vector sinks add file or HTTP event delivery.

## Usage

```rust
use crux_types::budget::Budget;
use crux_types::crux_value::Crux;

fn inspect<T: serde::Serialize>(trace: &Crux<T>) {
    println!("{}", trace.to_trace_json());
    println!("{}", trace.to_mermaid());
    let _limit = Budget::tokens(4_000);
}
```

## Key API

- `Crux<T>`: result fused with agent identity, timestamps, steps, and child traces.
- `Step`, `StepKind`, `StepStatus`, `CitedFinding`: causal execution records.
- `CruxId`, `TaskId`: prefixed ULID identifiers.
- `CruxErr`: serializable execution errors with transient-error classification.
- `Budget`, `BudgetTracker`, `BudgetUsage`, `HandlerUsage`, `UsdAmount`: typed resource limits.
- `Emission`, `EventSink`, `MultiSink`, `JsonlWriter`, vector sinks, and `MessageRouter`: event
  transport vocabulary and adapters.
- `PlanRule` and `RulePlanner`: serializable keyword-to-handler planning rules.
- `Priority`, `TaskLabel`, `DependencyKind`, and `RecoveryKind`: shared task/recovery schema.

`Crux<T>` exposes result extraction, causal/delegation queries, success counts, presentation JSON,
Mermaid rendering, and type-erased snapshots.

## Features

| Feature | Default | Effect |
| --- | --- | --- |
| `test-utils` | no | Exposes trace and step fixture builders. |
| `miette` | no | Adds diagnostic integration for `CruxErr`. |
| `vector-file` | no | Marks the file-backed vector sink feature surface. |
| `vector-http` | no | Enables HTTP vector delivery through Reqwest and Tokio. |

## Implementation status

Most public records derive serde traits. `VectorFileSink` writes JSONL locally; `VectorHttpSink` is
feature-gated. Kani proof code is `cfg(kani)`. No checked-in source is generated.

## Development and testing

```console
cargo nextest run -p crux-types
cargo nextest run -p crux-types --all-features
cargo clippy -p crux-types --all-targets --all-features -- -D warnings
```

Tests cover serde round trips, IDs, budget arithmetic, trace views, Mermaid output, event routing,
and optional sinks.
