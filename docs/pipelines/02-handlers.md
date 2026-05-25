# Handlers

A handler is what a step actually does. You specify it with the
`handler:` field. Arguments go in `args:`.

```yaml
- step: read_config
  handler: fs::read
  args:
    path: "config.toml"
```

## Shell

Run commands and capture output.

| Handler | Behavior |
|---------|----------|
| `shell::exec` | Run command, always succeeds (ignores exit code) |
| `shell::capture` | Run command, fail the step on non-zero exit |

```yaml
- step: list_files
  handler: shell::capture
  args:
    cmd: "ls -la src/"
```

Output shape: `{ "exit_code": 0, "stdout": "...", "stderr": "" }`

## Filesystem

| Handler | Args | Behavior |
|---------|------|----------|
| `fs::read` | `path` | Read a file to string |
| `fs::write` | `path`, `content` | Write a string to a file |
| `fs::glob` | `pattern` | Match files by glob pattern |
| `fs::exists` | `path` | Check if a path exists |

```yaml
- step: check_lockfile
  handler: fs::exists
  args:
    path: "Cargo.lock"
```

## Git

| Handler | Args | Behavior |
|---------|------|----------|
| `git::status` | -- | `git status --porcelain` |
| `git::diff` | `revision` | `git diff [revision]` |
| `git::log` | `count` | Recent commits as `hash\tsubject` |
| `git::staged_files` | -- | `git diff --cached --name-only` |

```yaml
- step: recent_commits
  handler: git::log
  args:
    count: 5
```

## JSON

Transform JSON data between steps.

| Handler | Args | Behavior |
|---------|------|----------|
| `json::pick` | `fields` | Extract named fields from input |
| `json::merge` | `with` | Merge a static object into input |
| `json::jq` | `expr` | Dot-path traversal (e.g. `".foo.bar"`) |

```yaml
- step: extract_name
  handler: json::jq
  args:
    expr: ".metadata.name"
```

Note: `json::jq` supports dot-path traversal only, not full jq syntax.
For complex transforms, use `shell::capture` with actual `jq`.

## Control flow

| Handler | Args | Behavior |
|---------|------|----------|
| `ctrl::noop` | -- | Pass input through unchanged |
| `ctrl::log` | `field`, `compact`, `pretty` | Log to stderr and pass through |
| `ctrl::assert` | `condition`, `message` | Fail the step if condition is falsy |

```yaml
- step: log_state
  handler: ctrl::log
  args:
    field: stdout     # log only this field (optional)
    pretty: true      # pretty-print JSON (optional)
    compact: true     # one-line JSON (optional)
```

## LLM

| Handler | Args | Behavior |
|---------|------|----------|
| `llm::invoke` | `prompt`, `provider`, `model` | Raw LLM completion |

```yaml
- step: summarize
  handler: llm::invoke
  args:
    prompt: "Summarize this text: {{input}}"
    provider: anthropic
    model: claude-sonnet-4-6
```

Requires `ANTHROPIC_API_KEY` or `OPENAI_API_KEY` in the environment.

For structured extraction, see
[LLM pipelines](./05-llm-pipelines.md) (requires `--features baml`).

## Full reference

See [Handlers and capabilities](../crux-capabilities.md) for the
complete list including text parsing, analysis, CI, review, and triage
handlers.

Next: [Combinators](./03-combinators.md) -- fan-out, piping, and
speculation.
