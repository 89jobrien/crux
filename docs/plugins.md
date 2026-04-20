# cruxx Plugins

Plugins extend cruxx pipelines with handlers for third-party
services. A plugin is any executable that speaks the cruxx plugin
protocol over stdin/stdout.

## Quick Start

1. Create a `~/.cruxx/plugins.toml`:

   ```toml
   [[plugin]]
   name = "github"
   path = "/usr/local/bin/cruxx-github"
   env = { GITHUB_TOKEN = "ghp_..." }
   ```

2. Run a pipeline that uses plugin handlers:

   ```bash
   cruxx run my-pipeline.cruxx
   ```

3. Or generate a pipeline that uses plugins:

   ```bash
   cruxx plan --goal "create a GitHub issue for each TODO"
   ```

## Plugin Protocol

Plugins communicate via newline-delimited JSON on stdin/stdout.

### Declare (host -> plugin)

Request:
```json
{"method":"Declare"}
```

Response:
```json
{
  "status": "Declare",
  "data": {
    "handlers": [
      {
        "name": "github::create_issue",
        "description": "Create a GitHub issue"
      }
    ]
  }
}
```

### Invoke (host -> plugin)

Request:
```json
{
  "method": "Invoke",
  "params": {
    "handler": "github::create_issue",
    "input": {"title": "Bug report", "body": "..."}
  }
}
```

Success response:
```json
{
  "status": "InvokeOk",
  "data": {"output": {"id": 42, "url": "..."}}
}
```

Error response:
```json
{
  "status": "InvokeErr",
  "data": {"error": "authentication failed"}
}
```

### Shutdown (host -> plugin)

Request:
```json
{"method":"Shutdown"}
```

Response:
```json
{"status":"ShutdownAck"}
```

## Writing a Plugin

A plugin is any binary that:

1. Reads newline-delimited JSON from stdin
2. Writes newline-delimited JSON to stdout
3. Handles `Declare`, `Invoke`, and `Shutdown` methods

See `crates/cruxx-plugin/tests/host.rs` and `crates/cruxx-plugin/tests/bridge.rs`
for working examples.

## Handler Naming

Plugin handlers use `namespace::action` format:
- `github::create_issue`
- `slack::post_message`
- `linear::create_ticket`
- `jira::transition_issue`

The namespace comes from the `name` field in `plugins.toml`.
