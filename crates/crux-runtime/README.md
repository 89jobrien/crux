# crux-runtime

Execution engine for Crux agents. It records causal traces while enforcing budgets, replay rules,
hooks, governance, approval, safety, trust, and task persistence.

## Architecture role

The runtime sits between the facade/macros and the serializable `crux-types` schema. `CruxCtx`
coordinates execution; ports such as `Context`, `RegistryBackend`, `ApprovalGate`, `SafetyPolicy`,
`AuditSink`, and `EventSink` keep infrastructure replaceable. `InMemoryBackend` is always available,
while redb persistence is optional.

## Usage

```rust
use crux_runtime::prelude::*;

async fn run(input: String) -> Crux<String> {
    let mut ctx = CruxCtx::new("uppercase");
    let result = ctx
        .step("uppercase", || async move { Ok(input.to_uppercase()) })
        .await;
    ctx.finalize(result)
}
```

## Key API

- `CruxCtx`: records `step`, confidence-aware steps, checkpoints, replay, output propagation, and
  event emission.
- Combinators: `delegate`, `speculate`, `pipe`, `pipe_with_recovery`, `join_all`, and
  `route_on_confidence`.
- `Agent`: typed async agent port with input/output types, budget, and lifecycle hooks.
- `TaskRegistry<B>` and `RegistryBackend`: optimistic task persistence with in-memory and optional
  redb adapters.
- `ReplayCache` and `ReplayMode`: strict ordinal matching or lenient forward name matching.
- `Budget`, `BudgetTracker`, `HandlerUsage`, and `UsdAmount`: limits and metering re-exported from
  `crux-types`.
- Governance ports: approval, audit, safety, policy composition, and decaying trust scores.

`crux_runtime::prelude::*` is the intended ergonomic import. Module paths remain public for focused
use and adapter implementations.

## Features

| Feature | Default | Effect |
| --- | --- | --- |
| `tokio-runtime` | yes | Enables Tokio/Futures-dependent execution. |
| `redb` | no | Enables `RedbBackend` and temporary redb databases. |
| `tracing` | no | Enables tracing spans around runtime operations. |

## Implementation status

Replay identity depends on step names and ordinals, so renaming steps changes cache compatibility.
`CruxCtx` is mutable and executes combinators through recorded steps. Wire types are re-exported,
not duplicated. Kani proof modules compile only under `cfg(kani)`; there is no generated source.

## Development and testing

```console
cargo nextest run -p crux-runtime
cargo nextest run -p crux-runtime --features redb,tracing
cargo clippy -p crux-runtime --all-targets --all-features -- -D warnings
```

Runtime unit tests cover combinators, hooks, replay, budgets, policies, registry conformance, and
trace recording; facade integration tests exercise public behavior end to end.
