# Crux Pipeline Capabilities

Native support status for cruxx-script pipeline step types and handlers.

## Pipeline Combinators

| Kind               | Pipeline key                       | Status                                                          |
| ------------------ | ---------------------------------- | --------------------------------------------------------------- |
| Single step        | `step:`                            | Supported                                                       |
| Sequential pipe    | `pipe:` + `stages:`                | Supported                                                       |
| Parallel fan-out   | `join_all:` + `arms:`              | Supported                                                       |
| Speculate (race)   | `speculate:` + `mode: first_ok`    | Supported                                                       |
| Speculate (pick)   | `speculate:` + `mode: pick_best`   | Partial -- requires `score` field in output                     |
| Confidence routing | `route_on_confidence:` + `routes:` | Supported -- handlers must use `HandlerOutput::with_confidence` |
| Delegation         | `delegate:`                        | Partial -- parses but no agents pre-registered                  |

Budget fields parsed: `tokens`, `calls`, `duration_ms`, `cost_cents`.

### Handler Registration

Two registry methods exist for registering handlers:

- `registry.handler(name, f)` -- handler returns `HandlerOutput`
  (with optional confidence)
- `registry.handler_value(name, f)` -- handler returns plain `Value`
  (auto-wrapped, confidence defaults to 1.0)

## Native Handlers

| Kind                | What it does                                                                |
| ------------------- | --------------------------------------------------------------------------- |
| `shell::exec`       | Run shell command, ignore exit code                                         |
| `shell::capture`    | Run shell command, fail on non-zero exit                                    |
| `fs::read`          | Read file to string                                                         |
| `fs::write`         | Write string to file (`path` + `content` args)                              |
| `fs::glob`          | Glob pattern match (`pattern` arg)                                          |
| `fs::exists`        | Check path existence                                                        |
| `git::staged_files` | `git diff --cached --name-only`                                             |
| `git::diff`         | `git diff [revision]`                                                       |
| `git::log`          | `git log -N --format=%H\t%s`                                                |
| `git::status`       | `git status --porcelain`                                                    |
| `json::pick`        | Extract named fields from input object                                      |
| `json::merge`       | Merge static `with` object into input                                       |
| `json::jq`          | Dot-path traversal only (not full jq)                                       |
| `ctrl::noop`        | Pass input through unchanged                                                |
| `ctrl::log`         | Log to stderr and pass through                                              |
| `ctrl::assert`      | Assert `args.condition` is truthy or fail                                   |
| `llm::invoke`       | Raw LLM completion (OpenAI/Anthropic/Ollama)                                |
| `container::run`    | Start a container from a `HarnessProfile` (`image`, `env`, `limits` args)   |
| `container::wait`   | Block until container exits; emits exit code and captured logs              |
| `harness::evolve`   | Run `EvolutionPlanner` against `RunMetrics` and apply resulting diff        |
| `harness::canary`   | Deploy a canary image alongside the current harness (`traffic_percent` arg) |
| `rx::run`           | Run a script from the rx registry by name (`name`, optional `args`)         |
| `rx::list`          | List all commands in the rx registry (optional `registry` path override)    |

## Text Parsing Handlers

Structured parsing of common CLI and file output formats.

| Kind                      | What it does                                                        |
| ------------------------- | ------------------------------------------------------------------- |
| `text::parse_vimgrep`     | Parse `file:line:col:text` (`rg --vimgrep`) into structured array   |
| `text::parse_jsonl`       | Parse newline-delimited JSON into array (skips invalid lines)       |
| `text::parse_frontmatter` | Extract YAML frontmatter (between `---` fences) from markdown       |
| `text::parse_diff`        | Parse unified diff into `[{file, hunks: [{old_start, new_start}]}]` |
| `text::parse_branch_list` | Parse `git branch` output into `[{name, current}]`                  |

## JSON Extended Handlers

| Kind                    | What it does                                                |
| ----------------------- | ----------------------------------------------------------- |
| `json::group_by`        | Group array items by a field (`args.key`), returns map      |
| `json::filter_nonempty` | Filter array items where `args.field` is non-empty/non-null |

## Analysis Handlers

Trace analysis and optimization for completed agent runs.

| Kind                           | What it does                                               |
| ------------------------------ | ---------------------------------------------------------- |
| `analysis::latency_profile`    | Flag steps exceeding 2x median wall-clock duration         |
| `analysis::token_spend`        | Accumulate token counts per step, identify top-3 consumers |
| `analysis::failure_clusters`   | Group failed steps by CruxErr kind                         |
| `analysis::replay_cache_hits`  | Compute ReplayCache hit/miss ratio per step name           |
| `analysis::tighten_budget`     | Suggest tighter Budget if spend > 80% (emits confidence)   |
| `analysis::compress_stages`    | Flag pipe stages consuming > 40% of total tokens           |
| `analysis::tune_retry`         | Suggest Recovery::Retry config for steps with > 2 failures |
| `analysis::patch_schema_check` | Validate a YAML patch string for syntax correctness        |
| `analysis::replay_dry_run`     | Re-run trace in lenient replay mode against a patch        |

## CI Handlers

CI log parsing and failure triage.

| Kind                    | What it does                                                   |
| ----------------------- | -------------------------------------------------------------- |
| `ci::compile_errors`    | Parse rustc error codes, messages, file locations from CI logs |
| `ci::clippy_violations` | Parse clippy warnings with lint names and file locations       |
| `ci::nextest_failures`  | Parse nextest FAIL lines and panic messages                    |
| `ci::deny_violations`   | Parse cargo-deny banned/license/advisory violations            |
| `ci::deduplicate_spans` | Collapse findings sharing the same file:line location          |
| `ci::classify_severity` | Rank findings: compile > deny > test > clippy                  |
| `ci::attach_owners`     | Map file paths to crate names via `cargo metadata`             |
| `ci::score_fixability`  | Heuristic auto-fix score as confidence (emits confidence)      |

## Review Handlers

PR review signal normalization and scoring.

| Kind                          | What it does                                             |
| ----------------------------- | -------------------------------------------------------- |
| `review::arch_boundary_check` | Detect domain->adapter/infra imports via `rg`            |
| `review::normalize_findings`  | Merge clippy, arch, and coverage findings into Finding[] |
| `review::apply_severity`      | Classify findings as blocking/suggestion/observation     |
| `review::compute_score`       | Reduce findings to 0.0-1.0 confidence score              |
| `review::approve`             | Run `gh pr review --approve`                             |

## Triage Handlers

Doob backlog processing and prioritization.

| Kind                         | What it does                                           |
| ---------------------------- | ------------------------------------------------------ |
| `triage::parse_repo_tags`    | Extract repo field from todo metadata                  |
| `triage::score_urgency`      | Score todos by age \* priority weight, sort descending |
| `triage::deduplicate_intent` | Cluster semantically similar titles via edit distance  |
| `triage::group_by_repo`      | Partition todos into per-repo buckets                  |

## Handlers (behind `--features baml`)

| Kind             | What it does                                                                         |
| ---------------- | ------------------------------------------------------------------------------------ |
| `llm::extract`   | BAML structured extraction (3 functions: `ExtractEntities`, `Summarize`, `Classify`) |
| `llm::decompose` | BAML spec decomposition into task list                                               |
| `llm::plan`      | BAML pipeline generation from natural language goal                                  |

## Known Gaps

| Area                   | Gap                                                                              |
| ---------------------- | -------------------------------------------------------------------------------- |
| `rx::install`          | installs scripts in a local registry                                             |
| `delegate:`            | Schema parses, runner dispatches, but `register_all` pre-registers no agents     |
| `route_on_confidence`  | `handler_value` handlers default to 1.0; use `handler` + `HandlerOutput` to emit |
| `speculate: pick_best` | Arms that don't emit `score` all tie at 0.0 (stub)                               |
| `llm::extract`         | Only 3 BAML functions wired; other function names fail                           |
| `json::jq`             | Dot-path only -- no filters, pipes, `select()`, `map()`                          |
