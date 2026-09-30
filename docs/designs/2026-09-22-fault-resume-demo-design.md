# Design: Fault and Resume Demo

## Goal

Provide a deterministic live demo that records a partial three-step HTTP execution, fails at the
final step, and resumes from the persisted trace with explicit replay provenance.

## Approved Approach

Add typed step provenance to the trace schema, expose it in CLI renderers, and drive a local HTTP
service implemented as a standalone `rust-script` from a runnable example script.

## Context Map

### Files to Modify

| File | Purpose | Changes Needed |
| --- | --- | --- |
| `crates/crux-types/src/step.rs` | Serializable step wire type | Add `StepOrigin` and `Step::origin`; test old/new JSON |
| `crates/crux-types/src/testing.rs` | Shared step fixtures | Mark generated fixtures as live |
| `crates/crux-runtime/src/recorder.rs` | Constructs live and replayed steps | Assign provenance at every recording path |
| `crates/crux-runtime/src/ctx.rs` | Constructs synthetic/delegated steps | Mark direct step construction as live |
| `crates/crux-runtime/src/speculation.rs` | Constructs speculative steps | Mark speculative records as live |
| `crates/crux-runtime/src/replay.rs` | Replay behavior tests | Update fixtures and assert replay provenance |
| `crates/crux-runtime/src/observability.rs` | JSONL and tracing export | Export step provenance |
| `crates/crux-schema/src/crux_value.rs` | Presentation trace schema | Include provenance and migrate test fixtures |
| `crates/crux-cli/src/bin/crux/output.rs` | Human run output | Render live and cached/replayed labels |
| `crates/crux-cli/src/bin/crux/trace.rs` | Trace timeline explorer | Add the provenance column |
| `crates/crux-cli/src/bin/crux/replay_debug.rs` | Detailed replay inspection | Print serialized provenance |
| `crates/crux-cli/src/bin/crux/run.rs` | CLI rendering fixtures | Migrate step fixtures |
| `crates/crux-improve/src/lib.rs` | Analysis test fixtures | Migrate step fixtures |
| `examples/fault_resume.crux` | Three-stage HTTP pipeline | Define context, evaluation, and finalization requests |
| `examples/fault_resume_server.rs` | Local HTTP service | Serve deterministic endpoints and persist request counts |
| `examples/fault_resume.sh` | Demo orchestrator | Run fault, inspect trace, replay, and verify counts |

### Dependencies

| File | Relationship |
| --- | --- |
| `crates/crux-runtime/src/lib.rs` | Re-exports step wire types through the prelude |
| `crates/crux-script/src/runner.rs` | Uses `CruxCtx` replay and handler invocation paths unchanged |
| `crates/crux-agentic/src/http.rs` | Existing policy-aware HTTP adapter used by the demo |
| `crates/crux-cli/src/bin/crux/run.rs` | Loads and saves replay traces through serde |

### Test Coverage

| Test location | Covers |
| --- | --- |
| `crates/crux-types/src/step.rs` | Step wire round trips and legacy defaulting |
| `crates/crux-runtime/src/ctx.rs` | Live and replayed step recording |
| `crates/crux-script/src/runner.rs` | Partial replay provenance, short-circuiting, and live resume |
| `crates/crux-cli/src/bin/crux/output.rs` | Human provenance labels |
| `crates/crux-cli/tests/trace_cli.rs` | Serialized trace timeline output |
| `crates/crux-cli/tests/replay_debug_cli.rs` | Detailed trace inspection output |

### Reference Patterns

| File | Pattern to Follow |
| --- | --- |
| `crates/crux-types/src/step.rs` | Serde-compatible wire enums and defaults |
| `crates/crux-runtime/src/recorder.rs` | Centralized live/replay record construction |
| `crates/crux-cli/src/bin/crux/output.rs` | Summary and verbose row formatting |
| `examples/plan_and_run.sh` | Repository-root-aware runnable demo script |

## Crate Ownership

- **Owner crate**: `crux-types` owns provenance because it is serialized trace vocabulary.
- **Runtime integration**: `crux-runtime` assigns provenance when recording a step.
- **Presentation integration**: `crux-schema` and `crux-cli` expose provenance to inspectors and
  humans.
- **Mechanical consumers**: `crux-improve` only requires fixture migration.

The change crosses more than three crates because `Step` is a shared wire type. The behavior is
owned by `crux-types` and `crux-runtime`; all other changes are presentation or struct-literal
migrations required by the approved explicit-field approach.

## Public API

### Types

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOrigin {
    #[default]
    Live,
    Replayed,
}
```

`Step<T>` gains this public field:

```rust
#[serde(default)]
pub origin: StepOrigin,
```

`StepOrigin` is re-exported from `crux_runtime::prelude` alongside `Step`, `StepKind`, and
`StepStatus`.

No new public traits or functions are introduced.

## Data Flow

1. The demo script starts the local `rust-script` HTTP service and receives its ephemeral port.
2. A live pipeline calls `/context` and `/evaluate`, then the server delays the first `/finalize`
   request beyond its timeout; the runtime records all three steps with `origin: live` and the CLI
   persists the failed trace.
3. The replay run loads that trace with the same pipeline input; completed outputs become replay
   cache hits with `origin: replayed`, while the failed final step has no cached output and executes
   live. The server completes the second `/finalize` request immediately.
4. CLI renderers display `[LIVE]` or `[CACHED / REPLAYED]`, and the server state proves that only
   `/finalize` received a second request.

## Hexagonal Boundaries

- **Network port**: the existing handler abstraction in `crux-script::HandlerRegistry`.
- **Network adapter**: the existing `crux-agentic` `http::request` handler.
- **Demo service**: `examples/fault_resume_server.rs`, external to production crates and run through
  `rust-script`.

No new production port or adapter is required.

## Demo Contract

- The server binds to `127.0.0.1:0`, writes its selected port to a file, and handles requests on
  separate threads so a timed-out request cannot block replay.
- `/context` and `/evaluate` return deterministic JSON and increment endpoint-specific counters.
- The first `/finalize` request delays beyond the client timeout; the second completes immediately,
  allowing fault and resume to use byte-identical input.
- The orchestrator stores generated inputs, traces, and server state under
  `target/fault-resume-demo/`.
- The orchestrator exits non-zero if the initial run unexpectedly succeeds, replay fails, or context
  and evaluation request counts exceed one.

## Out of Scope

- Calling a paid model provider or public internet endpoint.
- Adding a production trace store abstraction; the existing CLI JSON persistence remains the store.
- Changing strict or lenient replay matching semantics.
- Treating HTTP non-success status codes as handler failures.

## Risk

- [x] Public API change: adding a field breaks downstream `Step` struct literals.
- [x] Serialization change: new traces include `origin`; `#[serde(default)]` keeps old traces readable.
- [ ] New production dependency: none.
- [ ] Feature flag required: no.
- [x] CLI output change: human and trace-explorer rows gain provenance labels/columns.
- [x] Demo dependency: `rust-script` must be installed for the local service.
