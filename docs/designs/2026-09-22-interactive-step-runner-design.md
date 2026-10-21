# Design: Interactive Step Runner

## Goal

Add a truthful per-step pipeline boundary and an interactive browser console that can advance,
inspect, fault, and replay the demo using real Crux traces.

## Approved Approach

Add top-level `--through <step-name>` execution to `crux run`, then let the local Rust dashboard
server invoke the CLI and render persisted trace data after every boundary.

## Context Map

### Files to Modify

| File                                   | Purpose                           | Changes Needed                                                                 |
| -------------------------------------- | --------------------------------- | ------------------------------------------------------------------------------ |
| `crates/crux-script/src/runner.rs`     | Compiled pipeline execution       | Execute through a named top-level step; add replay tests                       |
| `crates/crux-cli/src/bin/crux/main.rs` | Run command arguments             | Add `--through`                                                                |
| `crates/crux-cli/src/bin/crux/run.rs`  | Pipeline dispatch and persistence | Route bounded execution through runner APIs                                    |
| `crates/crux-cli/tests/through_cli.rs` | CLI integration coverage          | Verify prefix traces, replay, and invalid boundaries                           |
| `examples/fault_resume_server.rs`      | Dashboard and local HTTP service  | Add bounded control worker, subprocess runner, dynamic traces, hardened routes |
| `examples/fault_resume.sh`             | Demo bootstrap                    | Pass server paths, drive headless mode, keep interactive mode open             |

### Dependencies

| File                                                     | Relationship                                                              |
| -------------------------------------------------------- | ------------------------------------------------------------------------- |
| `crates/crux-script/src/ir.rs`                           | Owns `TypedPipeline.steps`; no API shape change required                  |
| `crates/crux-types/src/step.rs`                          | Serialized status, origin, duration, confidence, output, and error fields |
| `examples/fault_resume.crux`                             | Three top-level handler steps used as boundaries                          |
| `docs/designs/2026-09-22-fault-resume-live-ui-design.md` | Superseded passive-dashboard design                                       |

### Test Coverage

| Test                                                            | Covers                                                     |
| --------------------------------------------------------------- | ---------------------------------------------------------- |
| `runner::tests::compiled_through_executes_named_prefix`         | Exact top-level execution boundary                         |
| `runner::tests::compiled_through_replay_only_executes_new_step` | Incremental replay behavior                                |
| `crates/crux-cli/tests/through_cli.rs`                          | CLI parsing, trace persistence, invalid boundary error     |
| `CRUX_DEMO_NO_OPEN=1 sh examples/fault_resume.sh`               | Control endpoints, step progression, trace details, replay |

### Reference Patterns

| File                                  | Pattern to Follow                                 |
| ------------------------------------- | ------------------------------------------------- |
| `crates/crux-script/src/runner.rs`    | Existing compiled execution and replay validation |
| `crates/crux-cli/src/bin/crux/run.rs` | Existing replay loading and trace persistence     |
| `examples/fault_resume_server.rs`     | Existing local HTTP state and dashboard routes    |

## Crate Ownership

- `crux-script` owns named top-level execution boundaries.
- `crux-cli` exposes the boundary through `crux run --through`.
- `examples/` owns orchestration, controls, state projection, and presentation.

## Public API

```rust
pub async fn Runner::run_compiled_through(
    &self,
    pipeline: &TypedPipeline,
    input: serde_json::Value,
    through: &str,
) -> Crux<serde_json::Value>;

pub async fn Runner::run_compiled_through_with_replay(
    &self,
    pipeline: &TypedPipeline,
    input: serde_json::Value,
    previous: &Crux<serde_json::Value>,
    mode: ReplayMode,
    through: &str,
) -> Crux<serde_json::Value>;
```

- Boundary names match top-level compiled step names exactly.
- The named step is included in execution.
- Nested control-flow nodes are atomic top-level boundaries.
- Unknown names return a failed trace with `CruxErr::StepFailed`.
- Existing full-run APIs remain unchanged.

## CLI Contract

- `crux run PIPELINE INPUT --through STEP` executes through `STEP` and persists a normal trace.
- `--through` composes with `--replay`, `--replay-mode`, and `--save-trace`.
- `--through` is rejected for Cruxfile target execution.
- Help text explicitly says “top-level step.”

## Interactive Server State

Private server state includes:

- phase: idle, running step, faulted, replaying, complete, or failed;
- current boundary and next boundary index;
- active and failed traces projected from persisted Crux JSON;
- request counters, command error, and worker-busy flag;
- control availability derived from state, not hard-coded in JavaScript.

Each projected trace step includes name, status, origin, duration, confidence, output, and error.

## Control Flow

1. `POST /control/next` executes the next boundary. It replays the current trace after Step A.
2. Step C's first live HTTP call times out and stores the failed trace.
3. `POST /control/replay` runs the full pipeline from the failed trace; Step A/B replay and Step C
   executes live.
4. `POST /control/run-all` resets, advances through all boundaries, pauses on the fault, then replays.
5. `POST /control/reset` clears traces, counters, and state when no command is running.
6. All control work is serialized through one bounded worker; concurrent commands receive 409.

## HTTP Safety

- Validate methods: GET for dashboard/state, POST for control and pipeline mutation endpoints.
- Return 405 for wrong methods and 404 for unknown routes.
- Reject request bodies above 64 KiB.
- Bind only to `127.0.0.1`.
- Do not spawn an unrestricted thread per request; only the serialized control worker and bounded
  finalize work may run asynchronously.
- Phase transitions are attempt-aware and terminal states do not regress.

## UI Contract

- Controls: Next Step, Replay Failed Step, Run All, Reset.
- Trace rows are built from `/state`, not hard-coded.
- Clicking a row opens serialized output/error details.
- Dynamic state and connection status use `aria-live`.
- Mobile trace rows collapse to a two-column layout without overflow.
- Interactive mode stays alive until Ctrl-C; headless mode runs assertions and exits.

## Out of Scope

- Pausing inside nested combinators.
- Remote dashboard access or authentication.
- General-purpose trace storage/query APIs.
- Editing pipeline input from the browser.

## Risk

- [x] Public API addition in `crux-script`; behavior is additive.
- [x] New CLI flag; existing invocations are unchanged.
- [ ] Production serialization change: none.
- [ ] New dependency: none.
- [x] Subprocess orchestration must avoid deadlocking while the pipeline calls the same local server.
- [x] Interactive mode is intentionally long-running and requires Ctrl-C cleanup.
