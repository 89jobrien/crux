# Design: Fault and Resume Live UI

## Goal

Show the fault-and-resume demo as a live browser dashboard that makes failure, replay cache hits,
and resumed HTTP execution visually obvious.

## Approved Approach

Extend the standalone Rust demo server with an embedded Bloomberg-terminal dashboard and a polled
JSON state endpoint; keep the feature isolated from production crates.

## Context Map

### Files to Modify

| File | Purpose | Changes Needed |
| --- | --- | --- |
| `examples/fault_resume_server.rs` | Local HTTP demo service | Serve dashboard and state JSON; track execution phase |
| `examples/fault_resume.sh` | Demo orchestration | Open dashboard and verify UI endpoints |

### Dependencies

| File | Relationship |
| --- | --- |
| `examples/fault_resume.crux` | Drives the three HTTP requests visualized by the dashboard |
| `target/fault-resume-demo/server-state.json` | Persists the same state exposed through `/state` |
| `target/fault-resume-demo/failed-trace.json` | Durable failed-run trace referenced by dashboard copy |
| `target/fault-resume-demo/resumed-trace.json` | Durable resumed-run trace referenced by dashboard copy |

### Test Coverage

| Test | Covers |
| --- | --- |
| `sh examples/fault_resume.sh` | Browser HTML route, state route, failure, replay, and final counters |
| Existing `crux-script` partial replay test | Cached steps remain unexecuted while failed step resumes live |

### Reference Patterns

| File | Pattern to Follow |
| --- | --- |
| `examples/fault_resume_server.rs` | Existing `rust-script`, threaded requests, atomic state writes |
| `crates/crux-cli/src/bin/crux/output.rs` | Canonical `LIVE` and `CACHED / REPLAYED` terminology |

## Crate Ownership

- **Owner**: the `examples/` demo owns the UI because it visualizes one example workflow.
- **Affected production crates**: none.
- **New dependencies**: none; the existing `tiny_http`, `serde`, and `serde_json` script
  dependencies are sufficient.

## Internal Types

The private demo state becomes:

```rust
#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
enum DemoPhase {
    #[default]
    Idle,
    Running,
    Faulting,
    Faulted,
    Complete,
}

#[derive(Clone, Copy, Default, Serialize)]
struct DemoState {
    phase: DemoPhase,
    context: u64,
    evaluate: u64,
    finalize: u64,
}
```

No public Rust API is added or changed.

## HTTP Surface

- `GET /` returns the embedded dashboard as UTF-8 `text/html`.
- `GET /state` returns the current `DemoState` as `application/json` without incrementing demo
  request counters.
- `GET /context`, `POST /evaluate`, and `POST /finalize` keep their existing behavior while updating
  `DemoPhase`.

## Data Flow

1. The shell starts the server and opens its root URL before executing the pipeline.
2. Pipeline requests update `DemoState`; each update is written atomically to disk.
3. Browser JavaScript polls `/state` every 100ms and maps the state to stage badges, request counters,
   fault details, and failed/resumed trace panels.
4. The first finalization reaches `faulted`; the shell waits long enough for that state to be visible
   before replay.
5. The second finalization reaches `complete`; the dashboard labels Steps 1-2
   `CACHED / REPLAYED` and Step 3 `LIVE`.

## Visual System

- Palette: black background, near-black raised panels, amber activity, acid-green success/replay,
  red fault, cool gray borders and secondary text.
- Typography: IBM Plex Mono when installed, then named coding-font fallbacks; tabular numerals
  throughout.
- Background: restrained line grid plus scanline texture and a narrow amber status band.
- Layout: masthead and run state, three-stage execution rail, four metrics, then side-by-side failed
  and resumed trace panels.
- Motion: one replay sweep across the first two stages; all motion disabled by
  `prefers-reduced-motion`.
- Responsive behavior: the rail and trace comparison stack vertically below 760px.

## Script Contract

- Open the dashboard with `/usr/bin/open` on macOS after the server writes its address.
- Skip browser launch when `CRUX_DEMO_NO_OPEN=1`.
- Require `/usr/bin/curl` and verify `/` contains the dashboard title and `/state` returns
  `phase: idle` before running the pipeline.
- Preserve server cleanup and signal handling.

## Out of Scope

- A reusable production trace viewer or `crux trace --html` command.
- WebSocket or server-sent-event infrastructure; bounded polling is sufficient for this demo.
- Reading arbitrary trace files from the browser.
- External font, script, or stylesheet CDNs.

## Risk

- [ ] Breaking API change: no.
- [ ] Serialization compatibility risk: no production format changes; demo state is private.
- [ ] New dependency: no.
- [x] Browser launch is platform-specific; headless mode remains available.
- [x] Inline HTML/CSS/JS increases the example server size but keeps the demo one-command and
  offline.
