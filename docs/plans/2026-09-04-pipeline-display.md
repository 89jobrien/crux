# Plan: Pipeline Display Metadata and Smart Rendering

## Goal

Make plain `crux run` render a concise pipeline summary, preserve `-v` as full verbose mode,
provide raw machine output through `--json`, and let pipelines declare friendly display labels.

## Context Map

### Files to Modify

| File | Purpose | Change |
| --- | --- | --- |
| `crates/crux-script/src/schema.rs` | Pipeline YAML schema | Add `PipelineDisplayDef`, `DisplayOutput`, and `PipelineDef::display` |
| `crates/crux-script/tests/pipeline.rs` | Schema integration tests | Cover display parsing and defaults |
| `crates/crux-script/src/validator.rs` | Nested pipeline construction | Initialize `display` in direct `PipelineDef` literals |
| `crates/crux-cli/src/bin/crux/registry.rs` | Human-readable rendering | Add smart summary and metadata-aware verbose trace |
| `crates/crux-cli/src/bin/crux/run.rs` | Output-mode dispatch | Route default, verbose, JSON, and quiet output |
| `crates/crux-cli/src/bin/crux/main.rs` | CLI definition | Add `--json` and output-mode conflicts |
| `crates/crux-cli/src/bin/crux/check.rs` | Validation registry setup | Initialize `display` in its `PipelineDef` literal |
| `docs/pipelines/01-first-pipeline.md` | Pipeline walkthrough | Document display metadata and all output modes |
| `docs/crux-syntax-reference.md` | Syntax reference | Add the `display` schema |
| `pipelines/crux/ci.crux` in `/Users/joe/dev/bamlish` | Bamlish CI pipeline | Add labels and remove redundant `ctrl::log` |
| `xtask/src/main.rs` in `/Users/joe/dev/bamlish` | Bamlish Crux launcher | Use `crux-cli` and default summary mode |

### Dependencies

- `crux-script::schema::PipelineDef` is consumed by the runner, validator, checker, and CLI.
- `crux-cli::run` loads `PipelineDef`, executes it, then passes `PipelineDef::display` and the
  resulting `Crux<Value>` to the selected renderer.
- `crux-cli::registry` reads runtime `Step` values and resolves presentation labels from the
  display metadata without changing the trace wire format.
- Bamlish supplies metadata in `pipelines/crux/ci.crux`; `cargo xtask crux-ci` invokes the Crux
  CLI without `-v`, so it receives the smart default summary.

### Existing Test Coverage

- `crates/crux-script/tests/pipeline.rs` covers YAML loading and pipeline execution.
- Inline tests in `crates/crux-cli/src/bin/crux/run.rs` cover raw JSON and verbose rendering.
- No current test covers display metadata, smart shell-output suppression, or `--json` parsing.

### Reference Patterns

- `BudgetDef` in `crates/crux-script/src/schema.rs` is the nearest optional pipeline-level schema.
- `render_trace` in `crates/crux-cli/src/bin/crux/registry.rs` is the existing pure renderer.
- `Cli::Run` in `crates/crux-cli/src/bin/crux/main.rs` owns current `-q` and `-v` flags.
- `docs/pipelines/01-first-pipeline.md` already documents output examples and verbosity.

### Risk

- `PipelineDef` is public; every direct struct literal must initialize the additive field.
- Plain `crux run` changes from raw JSON to text; scripts must migrate to `crux run --json`.
- `-v` must retain the complete trace and raw result regardless of pipeline display settings.
- Runtime trace serialization must remain unchanged; display metadata stays in `crux-script`.
- Cruxfile targets do not retain a single result value; reject `--json` for Cruxfiles rather than
  inventing an aggregate wire format in this change.
- Both repositories contain unrelated dirty files; stage only the exact files named per task.

## Architecture

- Crates affected: `crux-script`, `crux-cli`, and Bamlish's `xtask` integration.
- New types: `PipelineDisplayDef` and `DisplayOutput` in
  `crates/crux-script/src/schema.rs`; internal `OutputMode` in
  `crates/crux-cli/src/bin/crux/run.rs`.
- Data flow: pipeline YAML -> `PipelineDef::display` -> completed `Crux<Value>` -> selected CLI
  renderer -> terminal text or raw JSON.
- No runtime trace fields, persistence format, handler behavior, or external adapter changes.

## Tech Stack

- Rust 2024, MSRV 1.89.0.
- Existing `serde`, `serde-saphyr`, `serde_json`, `clap`, `crux-script`, and `crux-runtime` crates.
- No new dependency or feature flag.

## Preconditions

1. Work in `/Users/joe/dev/crux` on branch `feat/pipeline-display`.
2. Work in `/Users/joe/dev/bamlish` on branch `feat/pipeline-display`.
3. Run `git status --short` in each repository. Do not stage, discard, or rewrite unrelated dirty
   files. Use clean worktrees if the named implementation files already contain unrelated edits.
4. Preserve the design in `docs/designs/2026-09-04-pipeline-display-design.md`.

## Tasks

### Task 1: Parse Pipeline Display Metadata

**Crate**: `crux-script`
**File(s)**: `crates/crux-script/src/schema.rs`, `crates/crux-script/tests/pipeline.rs`
**Run**: `cargo nextest run -p crux-script display_metadata`

1. Add this failing integration test to `crates/crux-script/tests/pipeline.rs`:

   ```rust
   #[tokio::test]
   async fn display_metadata_parses_with_defaults() {
       let yaml = r#"
   pipeline: polished
   display:
     title: Polished Pipeline
     steps:
       analyze: Analysis
   steps:
     - step: analyze
       handler: analyzer
   "#;

       let pipeline = load(yaml).unwrap();
       let display = pipeline.display.as_ref().expect("display metadata");

       assert_eq!(display.title.as_deref(), Some("Polished Pipeline"));
       assert_eq!(display.output, crux_script::schema::DisplayOutput::Auto);
       assert_eq!(display.steps.get("analyze").map(String::as_str), Some("Analysis"));
   }
   ```

2. Run `cargo nextest run -p crux-script display_metadata`.
   Expected: compilation fails because `PipelineDef::display` and `DisplayOutput` do not exist.

3. Add these types immediately before `PipelineDef` in `crates/crux-script/src/schema.rs`:

   ```rust
   /// Human-facing presentation metadata for CLI pipeline output.
   #[derive(Debug, Clone, Default, Deserialize)]
   pub struct PipelineDisplayDef {
       /// Optional title used instead of the stable pipeline identifier.
       #[serde(default)]
       pub title: Option<String>,
       /// Controls whether successful final values appear in summary mode.
       #[serde(default)]
       pub output: DisplayOutput,
       /// Maps stable trace step names to human-facing labels.
       #[serde(default)]
       pub steps: IndexMap<String, String>,
   }

   /// Successful final-value visibility in the smart summary renderer.
   #[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
   #[serde(rename_all = "snake_case")]
   pub enum DisplayOutput {
       /// Show semantic values and suppress successful shell result envelopes.
       #[default]
       Auto,
       /// Always show the successful final value.
       Always,
       /// Never show a successful final value.
       Never,
   }
   ```

4. Add this field to `PipelineDef` between `vars` and `steps`:

   ```rust
   /// Optional human-facing presentation metadata.
   #[serde(default)]
   pub display: Option<PipelineDisplayDef>,
   ```

5. Run:

   ```text
   cargo nextest run -p crux-script display_metadata
   cargo clippy -p crux-script --all-targets -- -D warnings
   cargo fmt --all --check
   ```

6. Run `git branch --show-current`; require `feat/pipeline-display`.
7. Stage only the two task files and commit:

   ```text
   git add crates/crux-script/src/schema.rs crates/crux-script/tests/pipeline.rs
   git commit -m "feat(crux-script): add pipeline display metadata"
   ```

### Task 2: Update PipelineDef Consumers

**Crate**: `crux-script`
**File(s)**: `crates/crux-script/src/validator.rs`, `crates/crux-cli/src/bin/crux/check.rs`, `crates/crux-cli/src/bin/crux/run.rs`
**Run**: `cargo check -p crux-script -p crux-cli --all-targets`

1. Run the task command.
   Expected: compilation fails with missing field `display` in every direct `PipelineDef` literal.

2. Add `display: None,` immediately before `steps:` in these five literals:

   ```text
   crates/crux-script/src/validator.rs: nested_pipeline
   crates/crux-script/src/validator.rs: pipeline in validate_cruxfile
   crates/crux-cli/src/bin/crux/check.rs: empty_pipeline
   crates/crux-cli/src/bin/crux/run.rs: empty_pipeline
   crates/crux-cli/src/bin/crux/run.rs: tmp_pipeline
   ```

3. Also add `display: None,` to the temporary `PipelineDef` in
   `cmd_dry_run_cruxfile` in `crates/crux-cli/src/bin/crux/run.rs`.

4. Run:

   ```text
   cargo check -p crux-script -p crux-cli --all-targets
   cargo nextest run -p crux-script
   cargo clippy -p crux-script -p crux-cli --all-targets -- -D warnings
   ```

5. Run `git branch --show-current`; require `feat/pipeline-display`.
6. Stage only the three task files and commit:

   ```text
   git add crates/crux-script/src/validator.rs crates/crux-cli/src/bin/crux/check.rs crates/crux-cli/src/bin/crux/run.rs
   git commit -m "refactor: initialize optional pipeline display metadata"
   ```

### Task 3: Add the Smart Summary Renderer

**Crate**: `crux-cli`
**File(s)**: `crates/crux-cli/src/bin/crux/registry.rs`, `crates/crux-cli/src/bin/crux/run.rs`
**Run**: `cargo nextest run -p crux-cli summary_output`

1. Replace the inline test module's imports in `crates/crux-cli/src/bin/crux/run.rs` with:

   ```rust
   use super::*;
   use crux_runtime::prelude::{CruxId, Step};
   use crux_script::schema::{DisplayOutput, PipelineDisplayDef};
   use std::collections::HashMap;
   ```

2. Add this helper and failing tests after `ok_crux`:

   ```rust
   fn ok_step(name: &str, duration_ms: u64) -> Step {
       Step {
           name: name.to_string(),
           kind: StepKind::Plain,
           status: StepStatus::Ok,
           confidence: 1.0,
           started_at: chrono::Utc::now(),
           duration_ms,
           input_hash: 0,
           content_hash: None,
           output: None,
           error: None,
           attempt: 0,
           events: vec![],
           metadata: HashMap::new(),
           findings: vec![],
       }
   }

   fn display_metadata() -> PipelineDisplayDef {
       PipelineDisplayDef {
           title: Some("Bamlish CI".to_string()),
           output: DisplayOutput::Auto,
           steps: HashMap::from([("fmt_check".to_string(), "Formatting".to_string())])
               .into_iter()
               .collect(),
       }
   }

   #[test]
   fn summary_output_uses_display_labels_and_suppresses_shell_envelope() {
       let mut crux = ok_crux(json!({
           "exit_code": 0,
           "stdout": "all checks passed\n",
           "stderr": ""
       }));
       crux.steps.push(ok_step("fmt_check", 73));

       let out = render_summary(
           &crux,
           std::time::Duration::from_millis(73),
           Some(&display_metadata()),
       );

       assert!(out.contains("Bamlish CI"));
       assert!(out.contains("PASS"));
       assert!(out.contains("Formatting"));
       assert!(out.contains("1/1 checks passed"));
       assert!(!out.contains("exit_code"));
       assert!(!out.contains("all checks passed"));
   }

   #[test]
   fn summary_output_retains_semantic_result_in_auto_mode() {
       let crux = ok_crux(json!({"answer": 42}));
       let out = render_summary(
           &crux,
           std::time::Duration::from_millis(5),
           Some(&PipelineDisplayDef::default()),
       );

       assert!(out.contains("Output:"));
       assert!(out.contains(r#""answer": 42"#));
   }
   ```

3. Run `cargo nextest run -p crux-cli summary_output`.
   Expected: compilation fails because `render_summary` does not exist.

4. Change the import in `crates/crux-cli/src/bin/crux/registry.rs` to:

   ```rust
   use crux_script::{
       HandlerRegistry,
       schema::{DisplayOutput, PipelineDef, PipelineDisplayDef, StepDef},
   };
   ```

5. Add these pure helpers immediately before `render_trace`:

   ```rust
   fn display_title<'a>(crux: &'a Crux<Value>, display: Option<&'a PipelineDisplayDef>) -> &'a str {
       display
           .and_then(|metadata| metadata.title.as_deref())
           .unwrap_or(&crux.agent)
   }

   fn display_step_name<'a>(name: &'a str, display: Option<&'a PipelineDisplayDef>) -> &'a str {
       display
           .and_then(|metadata| metadata.steps.get(name))
           .map(String::as_str)
           .unwrap_or(name)
   }

   fn format_duration(duration: std::time::Duration) -> String {
       if duration.as_secs() >= 1 {
           format!("{:.2}s", duration.as_secs_f64())
       } else {
           format!("{}ms", duration.as_millis())
       }
   }

   fn is_shell_result(value: &Value) -> bool {
       value
           .as_object()
           .is_some_and(|object| {
               object.contains_key("exit_code")
                   && object.contains_key("stdout")
                   && object.contains_key("stderr")
           })
   }

   fn should_render_output(value: &Value, display: Option<&PipelineDisplayDef>) -> bool {
       match display.map_or(DisplayOutput::Auto, |metadata| metadata.output) {
           DisplayOutput::Auto => !is_shell_result(value),
           DisplayOutput::Always => true,
           DisplayOutput::Never => false,
       }
   }

   fn append_error(out: &mut String, error: &CruxErr) {
       out.push_str("\nFailure:\n");
       for line in error.to_string().lines() {
           out.push_str("  ");
           out.push_str(line);
           out.push('\n');
       }
   }

   /// Render concise human-facing pipeline output for the default CLI mode.
   pub fn render_summary(
       crux: &Crux<Value>,
       elapsed: std::time::Duration,
       display: Option<&PipelineDisplayDef>,
   ) -> String {
       let mut out = String::new();
       let status = if crux.value().is_ok() { "PASS" } else { "FAIL" };
       let title = display_title(crux, display);
       out.push_str(&format!("{title}  {status}  {}\n\n", format_duration(elapsed)));

       for step in &crux.steps {
           let icon = match step.status {
               StepStatus::Ok => "✓",
               StepStatus::Err => "✗",
               StepStatus::Rejected => "·",
               StepStatus::Skipped => "-",
           };
           let name = display_step_name(&step.name, display);
           let duration = format_duration(std::time::Duration::from_millis(step.duration_ms));
           out.push_str(&format!("  {icon} {name:<42} {duration:>8}\n"));
       }

       let passed = crux.steps.iter().filter(|step| step.status == StepStatus::Ok).count();
       out.push_str(&format!("\n{passed}/{} checks passed\n", crux.steps.len()));

       match crux.value() {
           Ok(value) if should_render_output(value, display) => {
               let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
               out.push_str(&format!("\nOutput:\n{pretty}\n"));
           }
           Ok(_) => {}
           Err(error) => append_error(&mut out, error),
       }

       out
   }
   ```

6. Change `render_trace` to accept metadata and resolve friendly names while preserving all raw
   output:

   ```rust
   pub fn render_trace(
       crux: &Crux<Value>,
       elapsed: std::time::Duration,
       display: Option<&PipelineDisplayDef>,
   ) -> String {
   ```

   Replace `crux.agent` in the pipeline heading with `display_title(crux, display)`. Before the
   existing per-step `format!`, bind:

   ```rust
   let name = display_step_name(&step.name, display);
   ```

   Then pass `name` instead of `step.name` to that `format!`. Do not apply `DisplayOutput` to
   `render_trace`; verbose mode always includes the complete raw `Output` section.

7. Update the existing verbose test call to pass `Some(&display_metadata())` and assert both
   `Trace:` and `Output:` remain present.

8. Run:

   ```text
   cargo nextest run -p crux-cli summary_output
   cargo nextest run -p crux-cli verbose_output
   cargo clippy -p crux-cli --all-targets -- -D warnings
   cargo fmt --all --check
   ```

9. Run `git branch --show-current`; require `feat/pipeline-display`.
10. Stage only the two task files and commit:

   ```text
   git add crates/crux-cli/src/bin/crux/registry.rs crates/crux-cli/src/bin/crux/run.rs
   git commit -m "feat(crux-cli): add smart pipeline summary"
   ```

### Task 4: Add Explicit JSON Output Mode

**Crate**: `crux-cli`
**File(s)**: `crates/crux-cli/src/bin/crux/main.rs`, `crates/crux-cli/src/bin/crux/run.rs`
**Run**: `cargo nextest run -p crux-cli output_mode`

1. Add this enum and selector after `RunConfig` in `crates/crux-cli/src/bin/crux/run.rs`:

   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   enum OutputMode {
       Summary,
       Verbose,
       Json,
       Quiet,
   }

   fn output_mode(config: &RunConfig<'_>) -> OutputMode {
       if config.verbose {
           OutputMode::Verbose
       } else if config.json {
           OutputMode::Json
       } else if config.quiet {
           OutputMode::Quiet
       } else {
           OutputMode::Summary
       }
   }
   ```

2. Add `pub json: bool,` between `quiet` and `verbose` in `RunConfig`.

3. Add a `config` test helper and failing mode test in the inline test module:

   ```rust
   fn config() -> RunConfig<'static> {
       RunConfig {
           pipeline_arg: Some("pipeline.crux"),
           target_or_input: None,
           check: false,
           target_flag: None,
           input_flag: None,
           plugins_path: None,
           quiet: false,
           json: false,
           verbose: false,
           dry_run: false,
           replay_path: None,
           replay_mode_str: "strict",
           save_trace_path: None,
           strict: false,
       }
   }

   #[test]
   fn output_mode_defaults_to_summary_and_preserves_explicit_modes() {
       let mut cfg = config();
       assert_eq!(output_mode(&cfg), OutputMode::Summary);

       cfg.json = true;
       assert_eq!(output_mode(&cfg), OutputMode::Json);

       cfg.json = false;
       cfg.verbose = true;
       assert_eq!(output_mode(&cfg), OutputMode::Verbose);

       cfg.verbose = false;
       cfg.quiet = true;
       assert_eq!(output_mode(&cfg), OutputMode::Quiet);
   }
   ```

4. In `Cli::Run` in `crates/crux-cli/src/bin/crux/main.rs`, replace the output flags with:

   ```rust
   /// Suppress all output except errors.
   #[arg(short, long, conflicts_with_all = ["verbose", "json"])]
   quiet: bool,
   /// Emit only the compact JSON result for machine consumption.
   #[arg(long, conflicts_with_all = ["quiet", "verbose"])]
   json: bool,
   /// Show the full trace envelope and raw final output.
   #[arg(short, long, conflicts_with_all = ["quiet", "json"])]
   verbose: bool,
   ```

5. Destructure `json` in the `Cli::Run` match and pass it into `RunConfig` between `quiet` and
   `verbose`.

6. Import `render_summary` beside `render_trace` at the top of `run.rs`:

   ```rust
   use crate::registry::{
       build_registry, collect_handler_names, render_summary, render_trace, warn_missing_env,
   };
   ```

7. Replace the regular pipeline output branch in `cmd_run` with:

   ```rust
   match output_mode(cfg) {
       OutputMode::Verbose => {
           print!("{}", render_trace(&crux, elapsed, pipeline.display.as_ref()));
       }
       OutputMode::Json => match render_default_output(&crux) {
           Ok(json) => println!("{json}"),
           Err(error) => eprintln!("{error}"),
       },
       OutputMode::Summary => {
           print!("{}", render_summary(&crux, elapsed, pipeline.display.as_ref()));
       }
       OutputMode::Quiet => {
           if let Err(error) = crux.value() {
               eprintln!("{error}");
           }
       }
   }

   if crux.value().is_err() {
       std::process::exit(1);
   }
   ```

8. At the start of `cmd_run_cruxfile`, after loading the Cruxfile, reject unsupported JSON mode:

   ```rust
   if cfg.json {
       eprintln!("error: --json is not supported for Cruxfile targets");
       std::process::exit(2);
   }
   ```

9. Rename `default_output_is_raw_json_of_result` to `json_output_is_raw_result` without changing
   its assertions. Update the verbose renderer test call to pass display metadata.

10. Run:

   ```text
   cargo nextest run -p crux-cli output_mode
   cargo nextest run -p crux-cli
   cargo clippy -p crux-cli --all-targets -- -D warnings
   cargo fmt --all --check
   ```

11. Run `git branch --show-current`; require `feat/pipeline-display`.
12. Stage only the two task files and commit:

   ```text
   git add crates/crux-cli/src/bin/crux/main.rs crates/crux-cli/src/bin/crux/run.rs
   git commit -m "feat(crux-cli): make JSON output explicit"
   ```

### Task 5: Document Display Metadata and Output Modes

**Crate**: `crux-cli`
**File(s)**: `docs/pipelines/01-first-pipeline.md`, `docs/crux-syntax-reference.md`, `docs/designs/2026-09-04-pipeline-display-design.md`
**Run**: `just ci`

1. Update the `hello.crux` example in `docs/pipelines/01-first-pipeline.md` to include:

   ```yaml
   display:
     title: Hello Pipeline
     output: auto
     steps:
       greet: Greeting
       timestamp: Timestamp
   ```

   Remove the `log_output` step because default summary rendering no longer needs `ctrl::log`.

2. Replace the default output example with:

   ```text
   Hello Pipeline  PASS  42ms

     ✓ Greeting                                       12ms
     ✓ Timestamp                                       8ms

   2/2 checks passed
   ```

3. Replace the verbosity examples with:

   ```text
   crux run hello.crux          # concise human-readable summary
   crux run hello.crux -v       # full trace and raw final output
   crux run hello.crux --json   # compact result JSON for scripts
   crux run hello.crux -q       # errors only
   ```

4. Add this section to `docs/crux-syntax-reference.md`:

   ```markdown
   ## Pipeline display metadata

   ```yaml
   pipeline: ci
   display:
     title: Project CI
     output: auto # auto | always | never
     steps:
       fmt_check: Formatting
       test: Tests
   ```

   Display metadata changes human-facing output only. Stable pipeline and step identifiers remain
   unchanged in saved traces and replay matching. `output` affects concise summary mode; `-v`
   always includes the complete final value.
   ```

5. Update `docs/designs/2026-09-04-pipeline-display-design.md` to explicitly state that `--json`
   is supported for regular pipelines only and Cruxfile JSON aggregation is out of scope.

6. Run `mdbook build docs` and `just ci`; verify both exit code 0.
7. Run `git branch --show-current`; require `feat/pipeline-display`.
8. Stage only the three documentation files and commit:

   ```text
   git add docs/pipelines/01-first-pipeline.md docs/crux-syntax-reference.md docs/designs/2026-09-04-pipeline-display-design.md
   git commit -m "docs: describe pipeline display modes"
   ```

### Task 6: Adopt Smart Output in Bamlish CI

**Crate**: `xtask`
**File(s)**: `/Users/joe/dev/bamlish/pipelines/crux/ci.crux`, `/Users/joe/dev/bamlish/xtask/src/main.rs`
**Run**: `cargo xtask crux-ci`

1. In `/Users/joe/dev/bamlish/pipelines/crux/ci.crux`, change the budget and add metadata after
   the header comments:

   ```yaml
   budget: { calls: 8 }
   display:
     title: Bamlish CI
     output: auto
     steps:
       fmt_check: Formatting
       clippy: Clippy
       cargo_check: Cargo check
       baml_check: BAML check
       test: Tests
       deny: Dependency audit
       machete: Unused dependencies
       conformance: Conformance
   ```

2. Delete the final `log_done` step from that pipeline:

   ```yaml
   - step: log_done
     handler: ctrl::log
     args:
       field: stdout
       pretty: true
   ```

3. Extract the current Crux command arguments into this pure helper without changing behavior:

   ```rust
   fn crux_ci_args() -> &'static [&'static str] {
       &[
           "run",
           "--quiet",
           "--manifest-path",
           "../crux/Cargo.toml",
           "-p",
           "crux-agentic",
           "--bin",
           "crux",
           "--",
           "run",
           "pipelines/crux/ci.crux",
           "-v",
       ]
   }
   ```

   Replace the argument literal in `crux_ci` with `crux_ci_args()`:

   ```rust
   fn crux_ci() -> Result<()> {
       run("cargo", crux_ci_args())
   }
   ```

   Add the test to the existing or new inline test module:

   ```rust
   #[test]
   fn crux_ci_uses_smart_default_output() {
       let args = crux_ci_args();
       assert!(args.contains(&"crux-cli"));
       assert!(!args.contains(&"-v"));
       assert!(!args.contains(&"--json"));
   }
   ```

4. Run the test before changing `crux_ci_args`.
   Expected: failure because the current launcher selects `crux-agentic` and `-v`.

5. Replace `crux_ci_args` with the green implementation:

   ```rust
   fn crux_ci_args() -> &'static [&'static str] {
       &[
           "run",
           "--quiet",
           "--manifest-path",
           "../crux/Cargo.toml",
           "-p",
           "crux-cli",
           "--bin",
           "crux",
           "--",
           "run",
           "pipelines/crux/ci.crux",
       ]
   }
   ```

6. Run from `/Users/joe/dev/bamlish`:

   ```text
   cargo test -p xtask crux_ci_uses_smart_default_output
   cargo xtask crux-ci
   cargo xtask pre-commit
   cargo test --workspace
   cargo run --quiet --manifest-path ../crux/Cargo.toml -p crux-cli --bin crux -- run pipelines/crux/ci.crux -v
   cargo run --quiet --manifest-path ../crux/Cargo.toml -p crux-cli --bin crux -- run pipelines/crux/ci.crux --json
   ```

   Verify the output has one concise `Bamlish CI` summary, eight friendly step labels, no duplicate
   conformance table, and no raw shell result envelope. Verify `-v` includes `Trace:` and `Output:`,
   while `--json` emits only the compact shell result object.

7. Run `git branch --show-current`; require `feat/pipeline-display`.
8. Stage only the two Bamlish files and commit:

   ```text
   git add pipelines/crux/ci.crux xtask/src/main.rs
   git commit -m "feat(ci): adopt smart Crux pipeline output"
   ```

## Compatibility Notes

- This is an intentional CLI behavior change: scripts parsing plain `crux run` output must add
  `--json`.
- `-v` remains the diagnostic mode and always includes the complete trace and raw final result.
- `--save-trace` JSON and replay matching remain byte-shape compatible because display metadata is
  never copied into `Crux<Value>`.
- `--json` is rejected for Cruxfile targets until a separate aggregate result schema is designed.

## Completion Criteria

- Display metadata parses with defaults and does not alter trace serialization.
- Plain pipeline execution matches the approved concise visual layout.
- `-v`, `--json`, and `-q` have distinct, tested behavior.
- Bamlish CI shows eight friendly checks once, with no duplicate conformance output.
- Crux and Bamlish quality gates pass without staging unrelated dirty files.
