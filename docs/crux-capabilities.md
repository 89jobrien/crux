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

- `registry.handler(name, f)` -- handler returns `HandlerOutput` (with optional confidence)
- `registry.handler_value(name, f)` -- handler returns plain `Value` (auto-wrapped, confidence defaults to 1.0)

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

## Handlers (behind `--features baml`)

| Kind             | What it does                                                                         |
| ---------------- | ------------------------------------------------------------------------------------ |
| `llm::extract`   | BAML structured extraction (3 functions: `ExtractEntities`, `Summarize`, `Classify`) |
| `llm::decompose` | BAML spec decomposition into task list                                               |
| `llm::plan`      | BAML pipeline generation from natural language goal                                  |

## Known Gaps

| Area                   | Gap                                                                                                      |
| ---------------------- | -------------------------------------------------------------------------------------------------------- |
| `delegate:`            | Schema parses, runner dispatches, but `register_all` pre-registers no agents                             |
| `route_on_confidence`  | `handler_value` handlers default to 1.0; use `handler` + `HandlerOutput::with_confidence` to emit scores |
| `speculate: pick_best` | Arms that don't emit `score` all tie at 0.0 (first arm wins)                                             |
| `llm::extract`         | Only 3 BAML functions wired; other function names fail                                                   |
| `json::jq`             | Dot-path only -- no filters, pipes, `select()`, `map()`                                                  |
| Domain analysis arms   | `ctrl::noop` placeholders in `examples/joe/` pipelines (aspirational templates). See ASPIRATIONAL header |
|                        | comments in each file for per-arm implementation guidance. Affected arms: `latency_profile`,             |
|                        | `dep_graph_analysis`, `arch_boundary_check`, `score_fixability`, `generate_patch`, `normalize_findings`, |
|                        | `deduplicate_intent`, `group_by_repo`, `tighten_budget`, `compress_prompt_stages`, `tune_retry_policy`,  |
|                        | `patch_schema_check`, `replay_dry_run`, `approve` (pr_review). See also: `json::jq` gap above.          |
