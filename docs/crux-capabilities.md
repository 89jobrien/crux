# Crux Pipeline Capabilities

Native support status for crux-script pipeline step types and handlers.

## Pipeline Combinators

| Kind               | YAML key                           | Status                                         |
| ------------------ | ---------------------------------- | ---------------------------------------------- |
| Single step        | `step:`                            | Supported                                      |
| Sequential pipe    | `pipe:` + `stages:`                | Supported                                      |
| Parallel fan-out   | `join_all:` + `arms:`              | Supported                                      |
| Speculate (race)   | `speculate:` + `mode: first_ok`    | Supported                                      |
| Speculate (pick)   | `speculate:` + `mode: pick_best`   | Partial -- requires `score` field in output    |
| Confidence routing | `route_on_confidence:` + `routes:` | Partial -- steps always emit confidence 1.0    |
| Delegation         | `delegate:`                        | Partial -- parses but no agents pre-registered |

Budget fields parsed: `tokens`, `calls`, `duration_ms`, `cost_cents`.

## Handlers (always available)

| Kind                | What it does                                   |
| ------------------- | ---------------------------------------------- |
| `shell::exec`       | Run shell command, ignore exit code            |
| `shell::capture`    | Run shell command, fail on non-zero exit       |
| `fs::read`          | Read file to string                            |
| `fs::write`         | Write string to file (`path` + `content` args) |
| `fs::glob`          | Glob pattern match (`pattern` arg)             |
| `fs::exists`        | Check path existence                           |
| `git::staged_files` | `git diff --cached --name-only`                |
| `git::diff`         | `git diff [revision]`                          |
| `git::log`          | `git log -N --format=%H\t%s`                   |
| `git::status`       | `git status --porcelain`                       |
| `json::pick`        | Extract named fields from input object         |
| `json::merge`       | Merge static `with` object into input          |
| `json::jq`          | Dot-path traversal only (not full jq)          |
| `ctrl::noop`        | Pass input through unchanged                   |
| `ctrl::log`         | Log to stderr and pass through                 |
| `ctrl::assert`      | Assert `args.condition` is truthy or fail      |
| `llm::invoke`     | Raw LLM completion (OpenAI/Anthropic/Ollama)   |

## Handlers (behind `--features baml`)

| Kind             | What it does                                                                         |
| ---------------- | ------------------------------------------------------------------------------------ |
| `llm::extract`   | BAML structured extraction (3 functions: `ExtractEntities`, `Summarize`, `Classify`) |
| `llm::decompose` | BAML spec decomposition into task list                                               |
| `llm::plan`      | BAML pipeline generation from natural language goal                                  |

## Known Gaps

| Area                   | Gap                                                                          |
| ---------------------- | ---------------------------------------------------------------------------- |
| `delegate:`            | Schema parses, runner dispatches, but `register_all` pre-registers no agents |
| `route_on_confidence`  | Steps always record `confidence: 1.0` -- no handler mechanism to set it      |
| `speculate: pick_best` | Arms that don't emit `score` all tie at 0.0 (first arm wins)                 |
| `llm::extract`         | Only 3 BAML functions wired; other function names fail                       |
| `json::jq`             | Dot-path only -- no filters, pipes, `select()`, `map()`                      |
| Domain analysis arms   | All `ctrl::noop` scaffolding in `examples/joe/` (aspirational)               |
