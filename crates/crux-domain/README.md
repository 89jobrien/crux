# crux-domain

Small domain vocabulary for deciding whether an agent step should execute, be denied, or be
simulated. It sits below the runtime and is suitable for consumers that do not need LLM clients or
the full execution engine.

## Architecture role

The crate owns planner-facing values and the synchronous `Planner` port. `CruxCtx` in
`crux-runtime` calls a planner before executing each step. An optional Tokio broadcast adapter
publishes `StepEvent` values without moving runtime policy into this crate.

## Usage

```rust
use crux_domain::plan_result::PlanResult;
use crux_domain::planner::{DenyAllPlanner, Planner};

let planner = DenyAllPlanner { reason: "read-only run".into() };
assert!(matches!(planner.next_action("write", 0), PlanResult::Deny { .. }));
```

## Key API

- `Action` and `StepIntent`: executable, skipped, or terminal step intent and priority.
- `PlanResult`: `Allow(Action)`, `Deny { reason }`, or `Simulate { output }`.
- `Planner`: synchronous `next_action(step_name, priority)` policy port.
- `PassthroughPlanner`, `DenyAllPlanner`, `SimulatePlanner`: built-in policies.
- `StepEvent`: serializable started, chunk, completed, and failed event vocabulary.
- `EventPipeline`, `EventSender`, `EventReceiver`: broadcast channel types behind a feature.

## Features

| Feature | Default | Effect |
| --- | --- | --- |
| `tokio-pipeline` | no | Enables the Tokio broadcast-backed `pipeline` module. |

## Implementation status

Core planner types are synchronous and serde-enabled. The optional event pipeline introduces only
Tokio's `sync` feature; there are no network or LLM dependencies. Events sent without subscribers
are dropped by the broadcast channel.

## Development and testing

```console
cargo nextest run -p crux-domain
cargo nextest run -p crux-domain --features tokio-pipeline
cargo clippy -p crux-domain --all-targets --all-features -- -D warnings
```

Unit tests cover serde shapes, each planner result, and broadcast delivery to one or more
subscribers.
