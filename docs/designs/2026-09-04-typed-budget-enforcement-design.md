# Design: Typed USD and Step Budget Enforcement

## Goal

Enforce pipeline budgets expressed as maximum USD spend and actual handler invocations, with
handler-reported usage, fail-closed accounting, and structured Miette CLI diagnostics.

## Approved Approach

Use `budget: { usd: 1.00, steps: 8 }`, count every attempted handler invocation as one step, and
require handlers to report actual cost after execution. Crossing the USD cap is a soft-cap failure:
the completed invocation may exceed the configured amount once before execution stops.

## Context Map

### Files to Modify

| File | Responsibility | Change |
| --- | --- | --- |
| `crates/crux-types/src/budget.rs` | Budget domain types and accounting | Add fixed-point USD and dimension-specific usage tracking |
| `crates/crux-types/src/error.rs` | Serializable domain errors | Add unreported-cost, step-limit, and USD-limit diagnostics |
| `crates/crux-runtime/src/context.rs` | Runtime context port | Add typed usage-recording methods |
| `crates/crux-runtime/src/ctx.rs` | Runtime accounting implementation | Count invocations and record handler usage |
| `crates/crux-script/src/handler_output.rs` | Handler execution boundary | Add outcome-plus-usage execution reports |
| `crates/crux-script/src/registry.rs` | Handler adapter registration | Add explicit free and metered registration methods |
| `crates/crux-script/src/runner.rs` | Pipeline orchestration | Translate schema budgets and enforce usage around invocations |
| `crates/crux-script/src/schema.rs` | YAML adapter | Parse canonical `usd`/`steps` and compatibility fields |
| `crates/crux-script/src/validator.rs` | Configuration diagnostics | Reject conflicting canonical and compatibility values |
| `crates/crux-stdlib/src/shell.rs` | Free shell adapter | Explicitly report zero USD for shell handlers |
| `crates/crux-cli/Cargo.toml` | CLI dependencies | Enable Miette diagnostics for `CruxErr` |
| `crates/crux-cli/src/bin/crux/run.rs` | CLI composition root | Render Miette for human modes and JSON errors for `--json` |
| `pipelines/crux/ci.crux` in Bamlish | CI pipeline configuration | Use `budget: { usd: 0.00, steps: 8 }` |

### Dependency Edges

- `crux-types` owns money, usage, limits, and domain errors without depending on adapters.
- `crux-runtime` implements the existing `Context` port using the typed budget tracker.
- `crux-script` adapts YAML values and handler execution reports into domain usage.
- `crux-stdlib` opts shell handlers into the explicit-free adapter path.
- `crux-cli` renders domain diagnostics; it does not define accounting policy.

### Existing Coverage

- `crates/crux-types/src/budget.rs` tests scalar and combined budget tracking.
- `crates/crux-runtime/src/ctx.rs` tests budget hook behavior.
- `crates/crux-script/src/schema.rs` tests compound YAML budget parsing.
- `crates/crux-script/src/handler_output.rs` tests output construction and confidence behavior.
- No current test distinguishes usage dimensions, unreported cost, failed-handler cost, or USD
  decimal parsing.

### Risk

- Existing `BudgetTracker::consume(u64)` cannot enforce mixed dimensions correctly; retain it only
  as a deprecated compatibility path and stop using it in pipeline execution.
- Existing handler registration APIs cannot report usage. They remain source-compatible but yield
  unreported cost under a USD budget.
- A post-execution USD report creates a soft cap, not a pre-authorized hard cap.
- Failed paid calls may still incur cost, so usage must accompany both success and failure outcomes.
- Saved `CruxErr` JSON gains variants but existing variants remain wire-compatible.

## Crate Ownership

- **Domain owner**: `crux-types` owns `UsdAmount`, `BudgetUsage`, limits, and violations.
- **Runtime owner**: `crux-runtime` owns invocation timing and applies domain accounting.
- **Port adapter owner**: `crux-script` owns handler execution reports and registry conveniences.
- **Infrastructure adapter**: `crux-stdlib` marks known-free handlers explicitly.
- **Presentation adapter**: `crux-cli` renders Miette or JSON diagnostics.

## Public API

### Domain Types in `crux-types`

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct UsdAmount {
    micros: u64,
}

impl UsdAmount {
    pub const ZERO: Self;
    pub const fn from_micros(micros: u64) -> Self;
    pub const fn micros(self) -> u64;
    pub fn checked_add(self, other: Self) -> Option<Self>;
}

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

`Budget` gains canonical variants while retaining legacy variants for serialized compatibility:

```rust
pub enum Budget {
    Tokens { limit: u64 },
    Steps { limit: u64 },
    Duration { limit_ms: u64 },
    Usd { limit_micros: u64 },
    Calls { limit: u64 },
    CostCents { limit: u64 },
    Combined { budgets: Vec<Budget> },
}
```

`BudgetKind` gains `Steps` and `Usd`; `Calls` and `CostCents` remain compatibility values.

`BudgetTracker` gains dimension-specific methods:

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

`consume(u64)` remains deprecated and maps to step consumption only.

### Domain Errors in `crux-types`

`CruxErr` gains:

```rust
UnreportedCost {
    step: String,
    source: Option<Box<CruxErr>>,
},
StepBudgetExceeded {
    limit: u64,
    attempted: u64,
},
UsdBudgetExceeded {
    limit_micros: u64,
    actual_micros: u64,
    source: Option<Box<CruxErr>>,
},
```

All three implement Miette diagnostic codes and contextual help through the existing conditional
`Diagnostic` implementation.

### Runtime Port in `crux-runtime`

The `Context` trait gains typed accounting methods:

```rust
fn begin_budgeted_step(&mut self) -> Result<(), CruxErr>;
fn record_handler_usage(&mut self, step: &str, usage: HandlerUsage) -> Result<(), CruxErr>;
fn record_budget_duration(&mut self, duration: Duration) -> Result<(), CruxErr>;
```

The legacy `consume_budget(u64)` method remains deprecated for source compatibility.

### Handler Boundary in `crux-script`

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

`HandlerRegistry` gains explicit adapter registration methods while retaining current methods:

```rust
pub fn handler_free<F, Fut>(&mut self, name: impl Into<String>, handler: F);
pub fn handler_metered<F, Fut>(&mut self, name: impl Into<String>, handler: F);
pub fn handler_value_free<F, Fut>(&mut self, name: impl Into<String>, handler: F);
```

- Existing `handler` and `handler_value` wrap outcomes as `HandlerExecution::unreported`.
- `handler_free` and `handler_value_free` wrap outcomes as explicit zero-USD usage.
- `handler_metered` receives the full `HandlerExecution`, including usage on errors.

### YAML Adapter in `crux-script`

`BudgetDef` gains canonical fields:

```rust
pub usd: Option<UsdAmount>,
pub steps: Option<u64>,
```

Compatibility behavior:

- `calls` maps to `steps` when `steps` is absent.
- `cost_cents` maps exactly to USD micro-units when `usd` is absent.
- Supplying both canonical and legacy forms for one dimension is a validation error.
- USD input accepts non-negative values with at most six decimal places.

## Execution Semantics

1. Before every actual handler invocation, including retries and parallel arms, call
   `begin_budgeted_step`. Control nodes and skipped branches consume no steps.
2. Execute the handler and retain both its outcome and usage report.
3. Record elapsed duration and reported tokens by their own dimensions.
4. If a USD budget is active and `usage.usd` is absent, return `UnreportedCost`; preserve an
   underlying handler failure as the related source diagnostic.
5. Add reported USD usage. Equality with the limit succeeds; exceeding it returns
   `UsdBudgetExceeded` and rejects the handler result.
6. A failed attempt still consumes one step and its reported cost.
7. Explicit-free handlers report `Some(UsdAmount::ZERO)` and remain valid under a zero-USD budget.

## CLI Error Adapter

- `crux-cli` enables the `crux-types/miette` feature and adds its existing workspace `miette`
  dependency.
- Summary, verbose, and quiet modes render `miette::Report` to stderr once.
- Human renderers do not append duplicate hand-formatted error blocks.
- `--json` serializes `CruxErr` to stderr without ANSI decoration.
- Exit codes remain non-zero for every accounting or execution failure.

## Data Flow

1. YAML `usd` is parsed into fixed-point `UsdAmount`; `steps` becomes a `Budget::Steps` limit.
2. The runner asks the runtime budget port to begin an invocation.
3. A handler adapter returns `HandlerExecution` with outcome and usage.
4. Runtime records typed usage against matching budget dimensions only.
5. Domain violations become `CruxErr`; CLI renders them through Miette or JSON.

## Out of Scope

- Currency conversion or currencies other than USD.
- Preflight reservation; the selected USD policy is a post-execution soft cap.
- A global model pricing table. Paid adapters own their reported cost.
- Removing legacy `calls`, `cost_cents`, or scalar `consume_budget` APIs in this release.

## Risk

- [x] Public API additions across `crux-types`, `crux-runtime`, and `crux-script`.
- [x] Behavioral change: handlers using legacy registration fail closed under USD budgets.
- [x] Soft-cap limitation: one completed invocation may cross the USD maximum.
- [ ] New external dependency: none; Miette already exists in the workspace.
- [ ] Runtime trace schema change: none.
