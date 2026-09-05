# Design: Pipeline Display Metadata and Smart Rendering

## Goal

Make plain `crux run` produce concise, human-readable pipeline summaries without duplicate
command output or raw shell result envelopes, while retaining genuinely verbose, machine-readable,
and quiet modes.

## Approved Approach

Combine declarative pipeline display metadata with a smart default renderer. Keep `-v` as the
full trace and raw-output mode, and move the current machine-readable result to `--json`.

## Context Map

### Files to Modify

| File | Purpose | Changes Needed |
| --- | --- | --- |
| `crates/crux-script/src/schema.rs` | Pipeline YAML schema | Add pipeline display metadata types and field |
| `crates/crux-cli/src/bin/crux/registry.rs` | CLI result rendering | Add smart summary and metadata-aware verbose output |
| `crates/crux-cli/src/bin/crux/run.rs` | Pipeline execution entry point | Pass display metadata into the renderer |
| `crates/crux-cli/src/bin/crux/main.rs` | CLI argument definition | Add explicit `--json` output mode |
| `crates/crux-script/tests/pipeline.rs` | Schema integration tests | Verify display metadata parsing and defaults |
| `docs/crux-syntax-reference.md` | Pipeline syntax documentation | Document the `display` block |
| `pipelines/crux/ci.crux` in Bamlish | Bamlish CI pipeline | Add labels and remove duplicate logging step |
| `xtask/src/main.rs` in Bamlish | Crux CI launcher | Stop forcing verbose mode |

### Dependencies

| File | Relationship |
| --- | --- |
| `crates/crux-cli/src/bin/crux/check.rs` | Constructs an empty `PipelineDef` |
| `crates/crux-cli/src/bin/crux/run.rs` | Constructs temporary `PipelineDef` values |
| `crates/crux-script/src/validator.rs` | Constructs nested and test `PipelineDef` values |

### Test Coverage

| Test | Covers |
| --- | --- |
| `crates/crux-script/tests/pipeline.rs` | YAML parsing and metadata defaults |
| `crates/crux-cli/src/bin/crux/run.rs` | Verbose summary and output visibility |

### Risk

- `PipelineDef` is public; adding an optional field requires updating direct struct literals.
- Default output changes from raw JSON to human-readable text; scripts must opt into `--json`.
- `-v` remains verbose and continues to include the full trace and raw result.
- Saved trace JSON remains unchanged because display metadata is not added to runtime trace types.

## Crate Ownership

- **`crux-script`** owns declarative display metadata because it is part of pipeline syntax.
- **`crux-cli`** owns smart rendering because presentation is a CLI concern.
- **Bamlish** only supplies pipeline-specific labels and removes redundant `ctrl::log` output.

## Public API

### Types

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct PipelineDisplayDef {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub output: DisplayOutput,
    #[serde(default)]
    pub steps: IndexMap<String, String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayOutput {
    #[default]
    Auto,
    Always,
    Never,
}
```

`PipelineDef` gains:

```rust
#[serde(default)]
pub display: Option<PipelineDisplayDef>,
```

### Functions

The CLI exposes separate smart and verbose renderers:

```rust
pub fn render_summary(
    crux: &Crux<Value>,
    elapsed: Duration,
    display: Option<&PipelineDisplayDef>,
) -> String;

pub fn render_trace(
    crux: &Crux<Value>,
    elapsed: Duration,
    display: Option<&PipelineDisplayDef>,
) -> String;
```

`RunConfig` gains:

```rust
pub json: bool,
```

No new trait or external dependency is required.

## Pipeline Syntax

```yaml
pipeline: bamlish_ci
display:
  title: Bamlish CI
  output: auto
  steps:
    fmt_check: Formatting
    clippy: Clippy
    cargo_check: Cargo check
```

- `title` overrides the pipeline identifier in human-readable output.
- `steps` maps stable trace step names to presentation labels.
- `output: auto` hides successful shell result envelopes but retains semantic output in summary mode.
- `output: always` includes final output in summary mode.
- `output: never` suppresses successful final output entirely.
- Failure diagnostics are always shown regardless of output mode.

## Smart Rendering Rules

1. Plain `crux run` renders a compact title, status, and total duration header.
2. Render one aligned row per trace step using metadata labels when available.
3. Render a final `N/N checks passed` line for successful pipelines.
4. In `auto` mode, detect shell result objects by `exit_code`, `stdout`, and `stderr`; suppress
   them on success.
5. On failure, print only the failing step's useful error text, without serializing the full
   shell envelope.
6. Preserve structured semantic results in `auto` mode under an `Output` section.
7. `crux run -v` renders the full trace and raw final output regardless of display visibility.
8. `crux run --json` emits only the compact JSON result previously emitted by default.
9. `--quiet`, saved traces, and exit codes remain unchanged.
10. `--json`, `--verbose`, and `--quiet` are mutually exclusive output modes.
11. Cruxfile execution rejects `--json` because targets do not currently expose one aggregate
    result value.

## Data Flow

1. `crux-script` deserializes optional display metadata with the pipeline definition.
2. The runner executes the pipeline without copying presentation data into runtime traces.
3. `crux-cli` selects summary, verbose, JSON, or quiet rendering from command-line flags.
4. The selected renderer resolves labels, classifies the final value, and writes terminal text.

## Out of Scope

- ANSI color and terminal capability detection.
- Per-step stdout streaming controls.
- Changes to saved trace schemas.
- A machine-readable aggregate format for Cruxfile target chains.
- Custom command wrappers; `$ARGUMENTS`, `$1`, and `$2` remain available if one is added later.

## Risk

- [x] Public API change: additive optional field on `PipelineDef` and `json` on `RunConfig`.
- [x] CLI compatibility change: callers consuming raw default output must add `--json`.
- [ ] Serialization format change: runtime traces are unchanged.
- [ ] New external dependency: none.
- [ ] Feature flag required: no.
