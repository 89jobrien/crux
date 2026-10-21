# Design: Automatic CLI Trace Persistence

## Goal

Automatically persist every executed `crux run` trace as replayable JSON under
`$HOME/.crux/traces/` so successful and failed runs remain inspectable without requiring an
explicit flag.

## Approved Approach

Use replay-ready files: each executed pipeline or Cruxfile target writes an independent,
timestamped JSON trace, while `--save-trace` remains an explicit destination override.

## Context Map

### Files to Modify

| File | Purpose | Changes Needed |
| --- | --- | --- |
| `crates/crux-cli/src/bin/crux/run.rs` | Pipeline execution and existing opt-in trace writes | Resolve automatic destinations, centralize persistence, and save every executed trace |
| `crates/crux-cli/src/bin/crux/main.rs` | `crux run` argument definitions | Describe `--save-trace` as an override for automatic persistence |
| `crates/crux-cli/tests/moa_review_cli_regressions.rs` | End-to-end output and exit-code contracts | Isolate `$HOME`, assert automatic success and failure traces, and account for save notices |
| `crates/crux-cli/tests/cruxfile_delegates.rs` | End-to-end Cruxfile execution | Isolate `$HOME` and verify target trace persistence |
| `README.md` | Top-level CLI usage | Document the automatic trace directory and explicit override |
| `docs/pipelines/01-first-pipeline.md` | First-run walkthrough | Explain that completed CLI runs save replayable traces |
| `docs/crux-syntax-reference.md` | CLI syntax card | Record automatic persistence and `--save-trace` override behavior |

### Dependencies

| File | Relationship |
| --- | --- |
| `crates/crux-types/src/id.rs` | Supplies the existing unique `CruxId` used in filenames |
| `crates/crux-types/src/crux_value.rs` | Supplies `Crux<Value>`, `started_at`, and the serialized trace envelope |
| `crates/crux-cli/Cargo.toml` | Already provides `chrono` and `serde_json`; no dependency change is needed |

### Test Coverage

| Test Location | Current Coverage | Required Coverage |
| --- | --- | --- |
| `crates/crux-cli/src/bin/crux/run.rs` | Output mode selection and rendering | Deterministic path generation, sanitization, and direct persistence helpers |
| `crates/crux-cli/tests/moa_review_cli_regressions.rs` | Regular pipeline streams and exit codes | Automatic files for successful, failed, replay, JSON, summary, verbose, and quiet runs |
| `crates/crux-cli/tests/cruxfile_delegates.rs` | Cruxfile stub and strict behavior | One replayable file per executed target under an isolated home directory |

### Reference Patterns

| File | Pattern to Follow |
| --- | --- |
| `crates/crux-cli/src/bin/crux/run.rs` | Existing pretty JSON serialization and `--save-trace` handling |
| `crates/crux-cli/src/bin/crux/registry.rs` | Existing `$HOME/.crux/` path resolution convention |
| `crates/crux-cli/tests/moa_review_cli_regressions.rs` | Existing subprocess-based CLI contract tests |

### Risk

- No public Rust API changes; all new helpers remain private to the CLI binary.
- No trace wire-format migration; files remain serialized `Crux<Value>` envelopes.
- Stderr changes for non-quiet `crux run` because automatic saves report their destination.
- Integration tests must isolate `$HOME` to avoid writing traces into the developer's real home.
- `README.md` already has unrelated working-tree changes and must be edited without overwriting them.

## Crate Ownership

- **Owner crate**: `crux-cli` because automatic persistence is CLI policy and filesystem I/O
  belongs at the application composition boundary.
- **Affected crates**: none. Existing types from `crux-runtime` and `crux-types` are consumed
  without modification.

## Public API

No public traits, types, or functions are added or changed.

### Internal Functions

```rust
fn trace_name_component(value: &str) -> String;

fn automatic_trace_path(
    home: &Path,
    trace: &Crux<Value>,
    pipeline_name: &str,
    target_name: Option<&str>,
) -> PathBuf;

fn persist_trace(trace: &Crux<Value>, path: &Path) -> std::io::Result<()>;

fn persist_automatic_trace(
    trace: &Crux<Value>,
    home: &Path,
    pipeline_name: &str,
    target_name: Option<&str>,
) -> std::io::Result<PathBuf>;
```

`automatic_trace_path` uses the trace's UTC `started_at`, a sanitized pipeline identity, an
optional sanitized target identity, and its unique `CruxId`. The resulting shape is:

```text
$HOME/.crux/traces/<timestamp>-<pipeline>[-<target>]-<crux-id>.json
```

Regular files use their file stem as the pipeline identity, stdin uses `stdin`, and Cruxfiles use
their declared project and executed target names. Sanitization preserves ASCII alphanumerics,
hyphens, and underscores; other runs become a single hyphen, with `pipeline` as the empty fallback.

## Runtime Behavior

1. Every pipeline or Cruxfile target that begins execution produces a `Crux<Value>` trace.
2. A regular pipeline with `--save-trace` uses that exact path; a Cruxfile retains its current
   `<override>.<target>.json` convention.
3. Without `--save-trace`, the CLI resolves `$HOME/.crux/traces/`, creates it recursively, and
   derives a unique replay-ready filename.
4. The CLI pretty-serializes the unchanged trace envelope and writes it before evaluating the
   run's final exit status.
5. Successful persistence reports `[crux] trace saved to <path>` on stderr unless `--quiet` is
   active.
6. Persistence failure prints a clear error and makes the command fail. For a Cruxfile dependency
   chain, it also prevents later targets from executing without trace coverage.
7. Failed pipeline executions are persisted before the process exits nonzero.
8. Replay executions produce a new trace file for the replayed run.
9. `--check` and `--dry-run` do not create traces because they do not execute a pipeline.

## Data Flow

1. **Source**: `Runner::run`, `Runner::run_with_replay`, or `Runner::run_target` returns a
   `Crux<Value>`.
2. **Destination selection**: explicit `--save-trace` wins; otherwise the CLI combines `$HOME`,
   trace metadata, and sanitized run identity into an automatic path.
3. **Transform**: `serde_json::to_string_pretty` serializes the unchanged trace envelope.
4. **Sink**: `std::fs` creates the automatic trace directory and writes one JSON file.
5. **Result**: the CLI renders its existing stdout mode, reports the saved path on stderr when
   allowed, and returns the combined execution/persistence status.

## Hexagonal Boundaries

- **Port**: no new domain port is required. Trace persistence is application-level CLI policy,
  not runtime domain behavior.
- **Adapter**: private `crux-cli::run` helpers perform local filesystem path resolution and writes.
- **Dependency direction**: `crux-cli` continues depending inward on runtime and wire types; no
  crate depends on CLI persistence code.

## Error Handling

- Missing `$HOME`, directory creation failure, serialization failure, and file write failure are
  surfaced as concise CLI errors.
- Automatic persistence creates only `$HOME/.crux/traces/`; explicit override parent directories
  are not implicitly created, preserving current behavior.
- Trace-save errors never alter the serialized execution result and never masquerade as pipeline
  step failures.

## Compatibility

- `--save-trace <path>` remains supported and takes precedence over automatic naming.
- `--replay <path>` accepts both automatic and explicitly saved files without conversion.
- `--json` stdout remains the compact result value; persistence notices use stderr.
- `--quiet` remains silent for successful runs while still returning nonzero and printing an error
  if persistence fails.
- Existing trace JSON shape and replay matching remain unchanged.

## Out of Scope

- Automatic persistence for Rust API calls that do not use `crux run`.
- Retention limits, pruning, compression, indexing, or search commands.
- Combining Cruxfile target traces into one aggregate envelope.
- Task registry, checkpoint, artifact, or run-ID integration.
- Changing replay matching or trace serialization formats.

## Risk Summary

- [x] Breaking API changes: no.
- [x] New external dependency: no.
- [x] Feature flag required: no.
- [x] Serialization migration required: no.
- [ ] CLI stderr compatibility: intentional save notices require regression-test updates.
- [ ] Filesystem growth: accepted; approved retention policy keeps all traces indefinitely.
