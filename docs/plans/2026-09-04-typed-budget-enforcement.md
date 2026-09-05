# Plan: Typed USD and Step Budget Enforcement

## Goal

Enforce `budget: { usd, steps }` with handler-reported cost, actual invocation counting,
fail-closed accounting, compatibility aliases, and Miette CLI diagnostics.

## Context Map

### Files to Modify

| File | Responsibility | Required change |
| --- | --- | --- |
| `crates/crux-types/src/budget.rs` | Budget domain | Add fixed-point USD, typed usage, canonical dimensions |
| `crates/crux-types/src/error.rs` | Domain errors | Add accounting error variants and Miette diagnostics |
| `crates/crux-types/src/lib.rs` | Public exports | Export new budget types |
| `crates/crux-runtime/src/context.rs` | Runtime port | Add typed accounting methods |
| `crates/crux-runtime/src/ctx.rs` | Runtime implementation | Delegate typed accounting to `BudgetTracker` |
| `crates/crux-script/src/schema.rs` | YAML adapter | Parse `usd` and `steps`, retain legacy values |
| `crates/crux-script/src/validator.rs` | Config validation | Reject conflicting canonical and legacy fields |
| `crates/crux-script/src/handler_output.rs` | Handler boundary | Add `HandlerExecution` carrying outcome and usage |
| `crates/crux-script/src/registry.rs` | Handler adapters | Add free and metered registration methods |
| `crates/crux-script/src/lib.rs` | Public exports | Export `HandlerExecution` |
| `crates/crux-script/src/runner.rs` | Pipeline orchestration | Enforce step, duration, token, and USD usage |
| `crates/crux-stdlib/src/shell.rs` | Shell adapter | Explicitly report zero USD |
| `crates/crux-cli/Cargo.toml` | CLI features | Enable Miette diagnostics |
| `crates/crux-cli/src/bin/crux/registry.rs` | Human renderer | Stop appending duplicate error text |
| `crates/crux-cli/src/bin/crux/run.rs` | CLI composition root | Render Miette or serialized JSON errors |
| `/Users/joe/dev/bamlish/pipelines/crux/ci.crux` | Consumer config | Use `{ usd: 0.00, steps: 8 }` |

### Dependency Edges

- YAML -> `BudgetDef` -> canonical `Budget` -> `BudgetTracker`.
- `HandlerRegistry` -> `HandlerExecution` -> `Runner` -> `Context` accounting port.
- `CruxErr` -> optional Miette diagnostic -> CLI presentation adapter.
- Bamlish shell steps -> explicit-free shell adapter -> zero-USD usage report.

### Coverage and Gaps

- Existing budget tests only exercise one scalar applied to every dimension.
- Existing registry tests assume handlers return `Result<HandlerOutput, CruxErr>` directly.
- Every direct handler invocation in `runner.rs` must retain usage, including retry, fallback,
  pipe, join, route, and speculate paths.
- Parallel combinators already await all `join_all` futures; usage must be recorded before
  propagating the resulting error.
- Speculation records reports only for futures that complete; every dispatched arm still consumes
  a step before execution.

### Risk

- Public APIs grow in `crux-types`, `crux-runtime`, and `crux-script`.
- Legacy handler registration remains source-compatible but fails closed under a USD budget.
- USD is a post-execution soft cap and can be exceeded by one completed invocation.
- Existing `calls` and `cost_cents` files remain valid but cannot be combined with canonical fields.
- Unrelated dirty files in Crux and Bamlish must never be staged or rewritten.

## Architecture

- Crates affected: `crux-types`, `crux-runtime`, `crux-script`, `crux-stdlib`, `crux-cli`, Bamlish.
- Domain types: `UsdAmount`, `HandlerUsage`, `BudgetUsage`, canonical `Budget` variants.
- Port: typed accounting methods on `crux_runtime::Context`.
- Adapters: YAML budget conversion, handler registry wrappers, free shell registration, Miette CLI.
- Data flow: handler execution -> usage report -> runtime tracker -> domain violation -> CLI report.

## Tech Stack

- Rust 2024, MSRV 1.89.0.
- Existing `serde`, `serde-saphyr`, `miette`, `tokio`, and Crux workspace crates.
- No new dependency.

## Tasks

### Task 1: Add Fixed-Point USD and Typed Usage

**Crate**: `crux-types`
**File(s)**: `crates/crux-types/src/budget.rs`, `crates/crux-types/src/lib.rs`
**Run**: `cargo nextest run -p crux-types usd_amount`

1. Write tests proving `UsdAmount` parses `1.25` as `1_250_000` microdollars, formats as `$1.250000`,
   rejects negative/non-finite values, and distinguishes free from unreported handler usage.
2. Add these public APIs in `budget.rs`:

   ```rust
   #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
   pub struct UsdAmount {
       micros: u64,
   }

   impl UsdAmount {
       pub const ZERO: Self = Self { micros: 0 };
       pub const fn from_micros(micros: u64) -> Self;
       pub const fn micros(self) -> u64;
       pub fn checked_add(self, other: Self) -> Option<Self>;
   }

   impl<'de> Deserialize<'de> for UsdAmount;
   impl std::fmt::Display for UsdAmount;

   #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
   pub struct HandlerUsage {
       pub tokens: u64,
       pub usd: Option<UsdAmount>,
   }

   impl HandlerUsage {
       pub const fn free() -> Self;
       pub const fn metered(tokens: u64, usd: UsdAmount) -> Self;
       pub const fn unreported() -> Self;
   }

   #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
   pub struct BudgetUsage {
       pub steps: u64,
       pub tokens: u64,
       pub duration_ms: u64,
       pub usd: Option<UsdAmount>,
   }
   ```

3. Implement `Deserialize` with a visitor accepting numeric USD, multiplying by `1_000_000`,
   rounding only when the difference is below `1e-6`, and returning a Serde custom error for
   negative, non-finite, overflow, or more than six decimal places.
4. Re-export the three types from `crates/crux-types/src/lib.rs` through its existing budget exports.
5. Verify `cargo nextest run -p crux-types usd_amount`, `cargo clippy -p crux-types -- -D warnings`,
   and `cargo fmt --all --check`.
6. Commit only the two files with `feat(crux-types): add typed USD usage`.

### Task 2: Make Budget Tracking Dimension-Specific

**Crate**: `crux-types`
**File(s)**: `crates/crux-types/src/budget.rs`, `crates/crux-types/src/error.rs`
**Run**: `cargo nextest run -p crux-types budget_tracker`

1. Replace tests that call `consume` across combined mixed units with tests that prove:
   - eight successful `begin_step` calls satisfy `Budget::steps(8)`;
   - the ninth returns `StepBudgetExceeded { limit: 8, attempted: 9 }`;
   - exact USD equality succeeds and one microdollar over fails;
   - missing USD usage under a USD limit returns `UnreportedCost`;
   - tokens and duration update only their matching counters.
2. Add canonical variants and constructors while retaining wire-compatible legacy variants:

   ```rust
   Budget::Steps { limit: u64 }
   Budget::Usd { limit_micros: u64 }
   Budget::steps(limit: u64) -> Budget
   Budget::usd(limit: UsdAmount) -> Budget
   BudgetKind::Steps
   BudgetKind::Usd
   ```

3. Replace `BudgetTracker::leaves: Vec<(u64, u64)>` with private counters containing
   `kind`, `limit`, and `used`. Normalize `Calls` to `Steps` and `CostCents` to USD microdollars
   while constructing counters.
4. Add:

   ```rust
   pub fn begin_step(&mut self) -> Result<(), CruxErr>;
   pub fn record_handler_usage(
       &mut self,
       step: &str,
       usage: HandlerUsage,
   ) -> Result<(), CruxErr>;
   pub fn record_duration(&mut self, duration: Duration) -> Result<(), CruxErr>;
   pub fn usage(&self) -> BudgetUsage;
   ```

5. `begin_step` checks `used + 1` before mutating. `record_handler_usage` first rejects `usd: None`
   when a USD counter exists, then adds token and USD values with checked arithmetic. Equality is
   allowed; values greater than limits return errors after recording actual usage.
6. Keep `consume(u64)` with `#[deprecated(note = "use typed budget accounting methods")]` and map
   it only to repeated step consumption.
7. Add these `CruxErr` variants and update `Display`, `failed_step`, `is_transient`, Serde tests,
   and exhaustive matches:

   ```rust
   UnreportedCost { step: String, source: Option<Box<CruxErr>> },
   StepBudgetExceeded { limit: u64, attempted: u64 },
   UsdBudgetExceeded {
       limit_micros: u64,
       actual_micros: u64,
       source: Option<Box<CruxErr>>,
   },
   ```

8. Verify package tests, Clippy, and formatting; commit with
   `feat(crux-types): enforce typed budget dimensions`.

### Task 3: Parse Canonical and Compatibility Budget Fields

**Crate**: `crux-script`
**File(s)**: `crates/crux-script/src/schema.rs`, `crates/crux-script/src/validator.rs`, `crates/crux-script/src/runner.rs`
**Run**: `cargo nextest run -p crux-script budget`

1. Add schema tests for:

   ```yaml
   budget: { usd: 1.25, steps: 8 }
   ```

   Assert `usd.micros() == 1_250_000` and `steps == Some(8)`.
2. Add validation tests rejecting `{ usd: 1.00, cost_cents: 100 }` and
   `{ steps: 8, calls: 8 }` with diagnostics at `budget.usd` and `budget.steps`.
3. Extend `BudgetDef` with `usd: Option<UsdAmount>` and `steps: Option<u64>`; retain `tokens`,
   `calls`, `duration_ms`, and `cost_cents`.
4. Add pipeline- and Cruxfile-target budget validation before step validation. Canonical and legacy
   forms for the same dimension are mutually exclusive.
5. Update `budget_from_def` to prefer canonical fields and otherwise map:

   ```text
   steps.or(calls) -> Budget::steps
   usd -> Budget::usd
   cost_cents -> Budget::usd(UsdAmount::from_micros(cost_cents * 10_000))
   ```

   Use checked multiplication; invalid overflow becomes a validation error before execution.
6. Verify package tests, Clippy, and formatting; commit with
   `feat(crux-script): support USD and step budgets`.

### Task 4: Add Metered Handler Execution Reports

**Crate**: `crux-script`
**File(s)**: `crates/crux-script/src/handler_output.rs`, `crates/crux-script/src/registry.rs`, `crates/crux-script/src/lib.rs`
**Run**: `cargo nextest run -p crux-script handler_execution`

1. Add tests proving usage survives successful and failed outcomes, free handlers report
   `Some(UsdAmount::ZERO)`, and legacy handlers report `None`.
2. Add:

   ```rust
   #[derive(Debug, Clone)]
   pub struct HandlerExecution {
       pub outcome: Result<HandlerOutput, CruxErr>,
       pub usage: HandlerUsage,
   }

   impl HandlerExecution {
       pub fn success(output: HandlerOutput, usage: HandlerUsage) -> Self;
       pub fn failure(error: CruxErr, usage: HandlerUsage) -> Self;
       pub fn free(outcome: Result<HandlerOutput, CruxErr>) -> Self;
       pub fn unreported(outcome: Result<HandlerOutput, CruxErr>) -> Self;
   }
   ```

3. Change `BoxHandler` futures to return `HandlerExecution` rather than
   `Result<HandlerOutput, CruxErr>`.
4. Keep `handler`, `handler_value`, and `handler_value_with_metadata` signatures unchanged; wrap
   their completed futures with `HandlerExecution::unreported`.
5. Add `handler_free`, `handler_value_free`, `handler_free_with_metadata`, and
   `handler_metered`. Free methods accept existing result-returning closures; metered accepts a
   future returning `HandlerExecution`.
6. Re-export `HandlerExecution` in `lib.rs`.
7. Verify package tests, Clippy, formatting, and `just ci`; commit with
   `feat(crux-script): add metered handler boundary`.

### Task 5: Extend the Runtime Accounting Port

**Crate**: `crux-runtime`
**File(s)**: `crates/crux-runtime/src/context.rs`, `crates/crux-runtime/src/ctx.rs`
**Run**: `cargo nextest run -p crux-runtime budget`

1. Add runtime tests proving step, USD, token, and duration methods delegate independently and
   return the new domain errors.
2. Add to `Context`:

   ```rust
   fn begin_budgeted_step(&mut self) -> Result<(), CruxErr>;
   fn record_handler_usage(
       &mut self,
       step: &str,
       usage: HandlerUsage,
   ) -> Result<(), CruxErr>;
   fn record_budget_duration(&mut self, duration: Duration) -> Result<(), CruxErr>;
   ```

3. Implement all three on `CruxCtx` by delegating to `BudgetTracker`.
4. Keep `consume_budget` deprecated and route it to the tracker's compatibility method.
5. Update budget hook tests to trigger typed violations rather than applying one scalar to mixed
   dimensions.
6. Verify package tests, Clippy, and formatting; commit with
   `feat(crux-runtime): expose typed budget accounting`.

### Task 6: Enforce Usage for Plain Steps, Retries, and Fallbacks

**Crate**: `crux-script`
**File(s)**: `crates/crux-script/src/runner.rs`
**Run**: `cargo nextest run -p crux-script metered_step`

1. Add integration tests with `handler_metered` and `handler_value_free` proving:
   - a plain free step succeeds under `{ usd: 0.00, steps: 1 }`;
   - a legacy handler fails with `UnreportedCost`;
   - failed retries consume steps and reported USD;
   - `on_error` consumes another step and records its usage.
2. Change `run_step_once` to call `ctx.begin_budgeted_step()` immediately before `ctx.step`.
3. Store `HandlerExecution::usage` in a shared cell before returning its outcome through
   `ctx.step`. After `ctx.step` completes, call `record_budget_duration` and
   `record_handler_usage`, then return the original result only when accounting succeeds.
4. When accounting fails after the handler also failed, attach the original failure as the
   `source` on `UnreportedCost` or `UsdBudgetExceeded`.
5. Route `run_on_error` through `run_step_once` with no timeout so fallback behavior uses identical
   accounting.
6. Verify package tests, Clippy, and formatting; commit with
   `feat(crux-script): meter handler attempts`.

### Task 7: Enforce Usage Across Combinators

**Crate**: `crux-script`
**File(s)**: `crates/crux-script/src/runner.rs`
**Run**: `cargo nextest run -p crux-script metered_combinator`

1. Add one metering test each for `pipe`, `join_all`, `route_on_confidence`, and `speculate`.
   Assert actual handler invocations, not control nodes or skipped routes, consume steps.
2. For `pipe`, wrap each stage future with a usage cell; record each completed stage immediately
   after `ctx.pipe` returns.
3. For `join_all`, call `begin_budgeted_step` once per live arm before dispatch. Store each arm's
   report in an indexed cell. Await `ctx.join_all` without `?`, record every populated report, then
   propagate the combinator result.
4. For routing, call `begin_budgeted_step` only inside the selected route future and record only its
   populated usage cell after routing completes.
5. For speculation, call `begin_budgeted_step` for every dispatched arm. Record reports only for
   completed futures; canceled futures have no completed billable report. Preserve all completed
   reports before returning the selected result or error.
6. Replayed and skipped handlers consume neither steps nor USD because their futures are not
   dispatched.
7. Verify package tests, Clippy, and formatting; commit with
   `feat(crux-script): meter combinator handlers`.

### Task 8: Mark Shell Handlers Explicitly Free

**Crate**: `crux-stdlib`
**File(s)**: `crates/crux-stdlib/src/shell.rs`
**Run**: `cargo nextest run -p crux-stdlib shell`

1. Add a registry test invoking both shell handlers and asserting each execution reports
   `Some(UsdAmount::ZERO)` on success and failure.
2. Replace `handler_value_with_metadata` with `handler_value_free_with_metadata` for
   `shell::exec` and `shell::capture`.
3. Do not mark HTTP, LLM, plugin, or unknown handlers free in this task.
4. Verify package tests, Clippy, and formatting; commit with
   `feat(crux-stdlib): declare shell handlers free`.

### Task 9: Render Budget Errors with Miette

**Crate**: `crux-cli`
**File(s)**: `crates/crux-cli/Cargo.toml`, `crates/crux-cli/src/bin/crux/registry.rs`, `crates/crux-cli/src/bin/crux/run.rs`
**Run**: `cargo nextest run -p crux-cli budget_error`

1. Add tests proving human modes contain Miette diagnostic codes/help exactly once and JSON mode
   emits a serialized `CruxErr` object without ANSI text.
2. Add `miette = { workspace = true }` and enable `features = ["miette"]` on the direct
   `crux-types` dependency in `crux-cli/Cargo.toml`.
3. Add a single CLI adapter:

   ```rust
   fn render_human_error(error: &CruxErr) {
       eprintln!("{:?}", miette::Report::new(error.clone()));
   }

   fn render_json_error(error: &CruxErr) {
       match serde_json::to_string(error) {
           Ok(json) => eprintln!("{json}"),
           Err(source) => eprintln!(r#"{{"kind":"serialization_error","message":"{source}"}}"#),
       }
   }
   ```

4. Remove `append_error` and the `Failure:` block from `render_summary`. Human renderers display
   trace/summary only; `cmd_run` renders the domain error once after output rendering.
5. Summary, verbose, and quiet call `render_human_error`; JSON calls `render_json_error`.
6. Add Miette codes `crux::unreported_cost`, `crux::step_budget_exceeded`, and
   `crux::usd_budget_exceeded`, with help naming the handler and formatted USD limits.
7. Verify package tests, Clippy, and formatting; commit with
   `feat(crux-cli): render budget diagnostics with miette`.

### Task 10: Adopt Canonical Budget Syntax in Bamlish

**Crate**: `xtask`
**File(s)**: `/Users/joe/dev/bamlish/pipelines/crux/ci.crux`, `/Users/joe/dev/bamlish/xtask/src/main.rs`
**Run**: `cargo xtask crux-ci`

1. Replace only the budget line with:

   ```yaml
   budget: { usd: 0.00, steps: 8 }
   ```

2. Preserve all eight CI steps and display metadata. Do not delete the budget or any CI check.
3. Keep `crux_ci_args` targeting `crux-cli` without `-v` or `--json`.
4. Regenerate ignored BAML client files with `cargo xtask generate` only if
   `baml_client/types.rs` is absent.
5. Run:

   ```text
   cargo xtask crux-ci
   cargo xtask pre-commit
   cargo test --workspace
   ```

6. Verify the concise output reports eight attempted shell handlers, `$0` actual spend, no
   unreported-cost error, and no duplicate raw shell envelope.
7. Commit only the two Bamlish files with `feat(ci): enforce USD and step budgets`.

## Compatibility Contract

- `calls` remains accepted as a compatibility spelling for `steps`.
- `cost_cents` remains accepted and converts exactly to USD microdollars.
- Canonical and compatibility spellings for one dimension cannot appear together.
- Existing handler APIs compile unchanged but are intentionally unreported under USD budgets.
- Existing serialized `Budget` and `CruxErr` variants remain deserializable.

## Completion Criteria

- Every actual handler invocation, including failed retries, consumes one step.
- Free handlers explicitly report zero; missing cost fails closed under USD limits.
- USD equality succeeds and post-execution overspend fails with actual usage retained.
- Miette renders human diagnostics once; JSON mode remains machine-readable.
- Bamlish CI uses `{ usd: 0.00, steps: 8 }` without deleting checks or budget metadata.
- Crux and Bamlish quality gates pass.
