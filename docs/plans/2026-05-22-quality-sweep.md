# Plan: Quality Sweep — Full Codebase Hardening

## Goal

Reduce the 1,401 findings (24.5% quality score) to under 200 actionable findings
by extracting mega-functions, naming magic numbers, removing dead code, and
splitting oversized modules. All changes are behaviour-preserving refactors.

## Context Map

### Files to Modify

| File                                  | Purpose                  | Changes Needed                                        |
| ------------------------------------- | ------------------------ | ----------------------------------------------------- |
| `crux-agentic/src/triage.rs`          | Triage pipeline handlers | Split 824-line `register()` into per-handler fns      |
| `crux-agentic/src/bin/crux.rs`        | CLI entrypoint           | Extract cmd\_\* fns into `cli/` submodules            |
| `crux-agentic/src/review.rs`          | Code review handlers     | Split 363-line `register()`                           |
| `crux-agentic/src/ci.rs`              | CI failure handlers      | Split 343-line `register()`                           |
| `crux-agentic/src/analysis.rs`        | Trace analysis handlers  | Split 394-line `register()`                           |
| `crux-runtime/src/ctx.rs`             | CruxCtx runtime          | Extract join_all, step_retryable, step_stream         |
| `crux-script/src/runner.rs`           | Pipeline runner          | Extract execute_step dispatch                         |
| `crux-stdlib/src/json.rs`             | JSON handlers            | Split register() + extract eval_jq                    |
| `crux-stdlib/src/text.rs`             | Text handlers            | Extract parse_diff into own fn (done), reduce nesting |
| `crux-agentic/src/container.rs`       | Container handlers       | Name magic numbers                                    |
| `crux-agentic/src/llm.rs`             | LLM handlers             | Extract DEFAULT_MAX_TOKENS const                      |
| `crux-planner/src/evolution.rs`       | Harness evolution        | Name magic numbers                                    |
| `crux-agentic/src/adapters/ollama.rs` | Ollama adapter           | Remove dead `list_models`                             |
| `crux-script/src/lib.rs`              | Script crate root        | Remove dead `load_cruxfile_file`                      |
| `crux-script/src/metadata.rs`         | Handler metadata         | Remove dead `ArgSchema::allow_extra`                  |
| `crux-plugin/src/discovery.rs`        | Plugin discovery         | Remove dead `default_path`                            |
| `crux-stdlib/src/ctrl.rs`             | Control flow handlers    | Remove dead `register_echo_agent`                     |
| `crux-improve/src/comparison.rs`      | Trace comparison         | Extract magic numbers                                 |
| `crux-improve/src/metrics.rs`         | Trace metrics            | Extract magic numbers, reduce fn length               |

### Dependencies (may need updates)

| File                           | Relationship                                       |
| ------------------------------ | -------------------------------------------------- |
| `crux-agentic/src/lib.rs`      | Calls `triage::register()`, `ci::register()`, etc. |
| `crux-agentic/src/handlers.rs` | Handler name constants — no changes needed         |
| `crux-script/src/lib.rs`       | Re-exports `Runner` — no signature changes         |
| `crux-runtime/src/lib.rs`      | Re-exports `CruxCtx` — no signature changes        |
| `crux-stdlib/src/lib.rs`       | Calls `json::register()`, `text::register()`       |

### Test Coverage

| Test File                            | Covers                                                  |
| ------------------------------------ | ------------------------------------------------------- |
| `crux-agentic/tests/triage.rs`       | parse_repo_tags, score_urgency, dedup, group_by_repo    |
| `crux-agentic/tests/ci_handlers.rs`  | compile_errors, clippy, nextest, deny, dedup, severity  |
| `crux-agentic/tests/review.rs`       | normalize, severity, score                              |
| `crux-agentic/tests/analysis.rs`     | latency, token_spend, clusters, budget, retry, compress |
| `crux-stdlib/tests/json_handlers.rs` | pick, merge, jq operations                              |
| `crux-script/tests/pipeline.rs`      | Runner execution paths                                  |
| `crates/crux/tests/`                 | Integration tests for CruxCtx combinators               |

### Risk

- [ ] All changes are internal refactors — no `pub` API signatures change
- [ ] No serialization format changes
- [ ] `register()` call sites unchanged (same fn name, same args)
- [ ] CLI output unchanged (only internal extraction of cmd\_\* bodies)
- [ ] Dead code removal may break doc examples — check docs/plans/\*.md

## Architecture

- **Crates affected**: crux-agentic, crux-runtime, crux-script, crux-stdlib,
  crux-planner, crux-plugin, crux-improve
- **No new types/traits** — pure extraction refactors
- **Pattern**: each `register()` mega-function becomes a thin orchestrator
  calling per-handler registration fns; each cmd\_\* body moves to a submodule

## Tech Stack

- Rust edition 2024, MSRV 1.88
- No new dependencies

---

## Tasks

### Task 1: Extract triage.rs handler registrations

**Crate**: `crux-agentic`
**File(s)**: `crates/crux-agentic/src/triage.rs`
**Run**: `cargo nextest run -p crux-agentic -- triage`

1. Add named constants at the top of `triage.rs` for all magic numbers:

    ```rust
    const PRIORITY_CRITICAL_WEIGHT: f64 = 4.0;
    const PRIORITY_HIGH_WEIGHT: f64 = 3.0;
    const PRIORITY_MEDIUM_WEIGHT: f64 = 2.0;
    const PRIORITY_LOW_WEIGHT: f64 = 1.0;

    const CONFIDENCE_BROKEN_SECRETS: f64 = 0.1;
    const CONFIDENCE_DIRENV_UNLOADED: f64 = 0.3;
    const CONFIDENCE_KEY_MISSING: f64 = 0.6;
    const CONFIDENCE_HEALTHY_SECRETS: f64 = 0.95;

    const MAX_HOOK_OVERHEAD_MS: f64 = 5000.0;

    const CONFIDENCE_ORPHANED_WORKTREES: f64 = 0.3;
    const MANY_BRANCHES_THRESHOLD: usize = 5;
    const CONFIDENCE_MANY_BRANCHES: f64 = 0.6;
    const CONFIDENCE_CLEAN_STATE: f64 = 0.9;

    const TODO_ISSUE_MATCH_THRESHOLD: f64 = 0.5;
    ```

2. Extract each handler registration closure into a standalone async fn:

    ```rust
    pub fn register(registry: &mut HandlerRegistry) {
        register_parse_repo_tags(registry);
        register_score_urgency(registry);
        register_deduplicate_intent(registry);
        register_group_by_repo(registry);
        register_diagnose_secrets(registry);
        register_diagnose_hooks(registry);
        register_diagnose_worktrees(registry);
        register_sync_todo_issues(registry);
        // ... one call per handler
    }

    fn register_parse_repo_tags(registry: &mut HandlerRegistry) {
        registry.handler_value_with_metadata(
            HandlerMetadata::new(handlers::TRIAGE_PARSE_REPO_TAGS)
                .describe("Extract repo tag from todo metadata.")
                .risk(RiskLevel::Low)
                .deterministic(true),
            |input: Value| async move {
                // existing body unchanged
            },
        );
    }
    ```

3. Verify:

    ```
    cargo nextest run -p crux-agentic -- triage  → all green
    cargo clippy -p crux-agentic -- -D warnings  → zero warnings
    ```

4. Run: `git branch --show-current`
   Commit: `git commit -m "refactor(crux-agentic): split triage.rs register() into per-handler fns"`

---

### Task 2: Extract ci.rs handler registrations

**Crate**: `crux-agentic`
**File(s)**: `crates/crux-agentic/src/ci.rs`
**Run**: `cargo nextest run -p crux-agentic -- ci`

1. Add named constants:

    ```rust
    const MAX_CONTEXT_LINES: usize = 5;
    const NEXTEST_SECTION_PARTS: usize = 3;
    const NEXTEST_INDEX_OFFSET: usize = 4;
    ```

2. Extract each handler registration into standalone fns (same pattern
   as Task 1): `register_compile_errors`, `register_clippy_violations`,
   `register_nextest_failures`, `register_deny_violations`,
   `register_deduplicate_spans`, `register_classify_severity`,
   `register_score_fixability`.

3. Move `parse_location()` helper to module scope (already a standalone fn).

4. Verify:

    ```
    cargo nextest run -p crux-agentic -- ci  → all green
    cargo clippy -p crux-agentic -- -D warnings  → zero warnings
    ```

5. Commit: `git commit -m "refactor(crux-agentic): split ci.rs register() into per-handler fns"`

---

### Task 3: Extract review.rs handler registrations

**Crate**: `crux-agentic`
**File(s)**: `crates/crux-agentic/src/review.rs`
**Run**: `cargo nextest run -p crux-agentic -- review`

1. Extract each handler into standalone fns:
   `register_arch_boundary_check`, `register_normalize_findings`,
   `register_apply_severity`, `register_compute_score`,
   `register_format_report`.

2. Verify:

    ```
    cargo nextest run -p crux-agentic -- review  → all green
    cargo clippy -p crux-agentic -- -D warnings  → zero warnings
    ```

3. Commit: `git commit -m "refactor(crux-agentic): split review.rs register() into per-handler fns"`

---

### Task 4: Extract analysis.rs handler registrations

**Crate**: `crux-agentic`
**File(s)**: `crates/crux-agentic/src/analysis.rs`
**Run**: `cargo nextest run -p crux-agentic -- analysis`

1. Add named constants:

    ```rust
    const SLOW_STEP_MULTIPLIER: f64 = 2.0;
    const TOP_TOKEN_SPEND_COUNT: usize = 3;
    const BUDGET_TIGHTEN_THRESHOLD: f64 = 0.8;
    const BUDGET_TIGHTEN_FACTOR: f64 = 1.1;
    const FLAKY_THRESHOLD: f64 = 0.4;
    ```

2. Extract each handler into standalone fns:
   `register_latency_profile`, `register_token_spend`,
   `register_failure_clusters`, `register_replay_cache_hit_ratio`,
   `register_tighten_budget`, `register_tune_retry`,
   `register_compress_stages`, `register_patch_schema_check`.

3. Verify:

    ```
    cargo nextest run -p crux-agentic -- analysis  → all green
    cargo clippy -p crux-agentic -- -D warnings  → zero warnings
    ```

4. Commit: `git commit -m "refactor(crux-agentic): split analysis.rs register() into per-handler fns"`

---

### Task 5: Split bin/crux.rs into CLI submodules

**Crate**: `crux-agentic`
**File(s)**: `crates/crux-agentic/src/bin/crux.rs`,
new `crates/crux-agentic/src/bin/crux/` directory
**Run**: `cargo nextest run -p crux-agentic`

1. Create module directory `crates/crux-agentic/src/bin/crux/`:

    ```
    crates/crux-agentic/src/bin/crux/main.rs   ← Cli enum + main()
    crates/crux-agentic/src/bin/crux/run.rs     ← cmd_run, cmd_run_cruxfile,
                                                    cmd_run_dispatch, cmd_dry_run_*
    crates/crux-agentic/src/bin/crux/plan.rs    ← cmd_plan, cmd_plan_rule,
                                                    cmd_plan_llm, format_plan_output
    crates/crux-agentic/src/bin/crux/check.rs   ← cmd_check
    crates/crux-agentic/src/bin/crux/list.rs    ← cmd_list
    crates/crux-agentic/src/bin/crux/util.rs    ← build_registry, print_trace,
                                                    warn_missing_env, format_handoff
    ```

2. Move each function to its submodule, keeping signatures identical.
   The `main.rs` dispatches to submodule functions.

3. Verify:

    ```
    cargo build -p crux-agentic --bin crux  → builds
    cargo nextest run -p crux-agentic       → all green
    cargo clippy -p crux-agentic -- -D warnings  → zero warnings
    ```

4. Commit: `git commit -m "refactor(crux-agentic): split bin/crux.rs into CLI submodules"`

---

### Task 6: Extract CruxCtx long methods

**Crate**: `crux-runtime`
**File(s)**: `crates/crux-runtime/src/ctx.rs`
**Run**: `cargo nextest run -p crux-runtime`

1. Extract `join_all` inner logic (the per-arm loop at ~line 550-660)
   into a private helper `fn build_join_arm_step(...)`.

2. Extract `step_retryable` retry loop (lines ~990-1060) into a private
   helper `fn execute_with_retries(...)`.

3. Extract `step_stream` chunk accumulation (lines ~1150-1220) into a
   private helper `fn accumulate_stream_chunks(...)`.

4. Extract `step_inner` body into two helpers: `fn resolve_replay_hit(...)`
   and `fn execute_and_record(...)`.

5. Verify:

    ```
    cargo nextest run -p crux-runtime  → all green
    cargo nextest run -p crux          → integration tests green
    cargo clippy -p crux-runtime -- -D warnings  → zero warnings
    ```

6. Commit: `git commit -m "refactor(crux-runtime): extract CruxCtx helper methods from long fns"`

---

### Task 7: Extract crux-script runner.rs execute_step

**Crate**: `crux-script`
**File(s)**: `crates/crux-script/src/runner.rs`
**Run**: `cargo nextest run -p crux-script`

1. Extract per-step-kind dispatch arms from `execute_step` (288 lines)
   into private helpers:

    ```rust
    async fn execute_plain_step(&self, ...) -> ...
    async fn execute_pipe(&self, ...) -> ...
    async fn execute_join_all(&self, ...) -> ...
    async fn execute_route(&self, ...) -> ...
    async fn execute_speculate(&self, ...) -> ...
    async fn execute_delegate(&self, ...) -> ...
    ```

2. `execute_step` becomes a match dispatcher calling these helpers.

3. Verify:

    ```
    cargo nextest run -p crux-script  → all green
    cargo clippy -p crux-script -- -D warnings  → zero warnings
    ```

4. Commit: `git commit -m "refactor(crux-script): extract execute_step dispatch into per-kind helpers"`

---

### Task 8: Split crux-stdlib json.rs

**Crate**: `crux-stdlib`
**File(s)**: `crates/crux-stdlib/src/json.rs`
**Run**: `cargo nextest run -p crux-stdlib -- json`

1. Extract `eval_jq` (80 lines, CC=18) into a new file
   `crates/crux-stdlib/src/jq.rs`. Make it `pub(crate)`.

2. Extract `traverse` helper into `jq.rs` alongside `eval_jq`.

3. Split handler registrations in `register()` (174 lines) into
   per-handler fns (same pattern as Tasks 1-4).

4. Verify:

    ```
    cargo nextest run -p crux-stdlib -- json  → all green
    cargo clippy -p crux-stdlib -- -D warnings  → zero warnings
    ```

5. Commit: `git commit -m "refactor(crux-stdlib): extract jq engine and split json handler registrations"`

---

### Task 9: Reduce text.rs parse_diff nesting

**Crate**: `crux-stdlib`
**File(s)**: `crates/crux-stdlib/src/text.rs`
**Run**: `cargo nextest run -p crux-stdlib -- text`

1. Extract the inner hunk-parsing loop body (lines ~163-240) into a
   private fn `fn process_diff_line(...)` that takes mutable state refs.

2. Collapse the duplicated `+++ b/` and `+++ ` branches into one with
   an `strip_prefix("+++ b/").or_else(|| strip_prefix("+++ "))` chain.

3. Verify:

    ```
    cargo nextest run -p crux-stdlib  → all green
    cargo clippy -p crux-stdlib -- -D warnings  → zero warnings
    ```

4. Commit: `git commit -m "refactor(crux-stdlib): reduce parse_diff nesting and extract helper"`

---

### Task 10: Name magic numbers in container.rs + llm.rs

**Crate**: `crux-agentic`
**File(s)**: `crates/crux-agentic/src/container.rs`,
`crates/crux-agentic/src/llm.rs`
**Run**: `cargo nextest run -p crux-agentic -- container`

1. In `container.rs`, add constants:

    ```rust
    const DEFAULT_MEMORY_MB: u64 = 512;
    const DEFAULT_CPU_MILLICORES: u64 = 1000;
    const DEFAULT_TIMEOUT_SECONDS: u64 = 300;
    ```

2. In `llm.rs`, add a single constant and use it in all three handlers:

    ```rust
    const DEFAULT_MAX_TOKENS: u32 = 1024;
    ```

3. Replace all bare literals with the named constants.

4. Verify:

    ```
    cargo nextest run -p crux-agentic  → all green
    cargo clippy -p crux-agentic -- -D warnings  → zero warnings
    ```

5. Commit: `git commit -m "refactor(crux-agentic): name magic numbers in container + llm handlers"`

---

### Task 11: Name magic numbers in evolution.rs + improve crate

**Crate**: `crux-planner`, `crux-improve`
**File(s)**: `crates/crux-planner/src/evolution.rs`,
`crates/crux-improve/src/comparison.rs`,
`crates/crux-improve/src/metrics.rs`
**Run**: `cargo nextest run -p crux-planner && cargo nextest run -p crux-improve`

1. In `evolution.rs`, add constants:

    ```rust
    const OOM_EXIT_CODE: i32 = 137;
    const MIN_MEMORY_BUMP_MB: u64 = 128;
    const MIN_MEMORY_BUMP_PRESSURE_MB: u64 = 64;
    const MIN_TIMEOUT_BUMP_SECONDS: u64 = 30;
    const DEFAULT_MEMORY_PRESSURE_THRESHOLD: f64 = 0.9;
    const DEFAULT_TIMEOUT_PRESSURE_THRESHOLD: f64 = 0.9;
    const DEFAULT_MEMORY_BUMP_FACTOR: f64 = 0.5;
    const DEFAULT_TIMEOUT_BUMP_FACTOR: f64 = 0.5;
    ```

2. In `comparison.rs`, add constants:

    ```rust
    const IMPROVEMENT_THRESHOLD: f64 = 0.05;
    const REGRESSION_THRESHOLD: f64 = -0.05;
    ```

3. In `metrics.rs`, add constants:

    ```rust
    const DEFAULT_CONFIDENCE_WEIGHT: f64 = 0.5;
    const SUCCESS_RATE_WEIGHT: f64 = 0.6;
    const CONFIDENCE_WEIGHT: f64 = 0.4;
    ```

4. Replace all bare literals with the named constants.

5. Verify:

    ```
    cargo nextest run -p crux-planner  → all green
    cargo nextest run -p crux-improve  → all green
    cargo clippy -p crux-planner -- -D warnings  → zero warnings
    cargo clippy -p crux-improve -- -D warnings  → zero warnings
    ```

6. Commit: `git commit -m "refactor(crux-planner,crux-improve): name magic numbers"`

---

### Task 12: Remove confirmed dead code

**Crate**: `crux-agentic`, `crux-script`, `crux-plugin`, `crux-stdlib`
**File(s)**: See list below
**Run**: `cargo nextest run`

1. Remove `OllamaAdapter::list_models` from
   `crates/crux-agentic/src/adapters/ollama.rs:28`.

2. Remove `load_cruxfile_file` from `crates/crux-script/src/lib.rs:43`.

3. Remove `ArgSchema::allow_extra` builder method from
   `crates/crux-script/src/metadata.rs:102`.
   Keep the `allow_extra` struct field (it is used).

4. Remove `TomlFileDiscovery::default_path` from
   `crates/crux-plugin/src/discovery.rs:30`.

5. Remove `register_echo_agent` from
   `crates/crux-stdlib/src/ctrl.rs:67`.
   Also remove the doc reference in
   `docs/plans/2026-05-21-reliability-and-dx.md` if it exists.

6. Verify:

    ```
    cargo build --all-targets  → builds
    cargo nextest run          → all green
    cargo clippy -- -D warnings  → zero warnings
    ```

7. Commit: `git commit -m "chore: remove confirmed dead code across 5 crates"`

---

## Execution Order

Tasks 1-4 are independent (different files in same crate). Run in parallel.
Task 5 depends on nothing. Can run in parallel with 1-4.
Task 6 is independent (different crate). Can run in parallel.
Task 7 is independent (different crate). Can run in parallel.
Task 8-9 are independent (different files in crux-stdlib). Run in parallel.
Tasks 10-11 are independent. Run in parallel.
Task 12 (dead code) should run last — after all refactors — to avoid
merge conflicts from files being restructured.

**Suggested waves:**

- Wave 1: Tasks 1, 2, 3, 4 (crux-agentic register() splits)
- Wave 2: Tasks 5, 6, 7 (bin split, ctx.rs, runner.rs)
- Wave 3: Tasks 8, 9, 10, 11 (stdlib, magic numbers)
- Wave 4: Task 12 (dead code cleanup)

## Pre-Save Checklist

- [x] Every requirement maps to at least one task
- [x] No placeholders or vague directives
- [x] Method names and types consistent across all tasks
- [x] Each task is 2-10 minutes of focused work
- [x] Each task ends with a commit
