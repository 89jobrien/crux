# crux Plugins

Plugins extend crux pipelines with handlers for third-party
services. A plugin is any executable that speaks the crux plugin
protocol over stdin/stdout.

## Quick Start

1. Create a `~/.crux/plugins.toml`:

   ```toml
   [[plugin]]
   name = "github"
   path = "/usr/local/bin/crux-github"
   env = { GITHUB_TOKEN = "ghp_..." }
   ```

2. Run a pipeline that uses plugin handlers:

   ```bash
   crux run my-pipeline.crux
   ```

3. Or generate a pipeline that uses plugins:

   ```bash
   crux plan --goal "create a GitHub issue for each TODO"
   ```

## Plugin Protocol

Plugins communicate via newline-delimited JSON on stdin/stdout.

### Declare (host -> plugin)

Request:

```json
{ "method": "Declare" }
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

#### Declaring a result type

A handler may add an `output_schema` to describe the shape of a successful
result. The host lifts it into the step's typed contract, so the compiler can
check consumers statically instead of treating the output as `Dynamic`:

```json
{
  "status": "Declare",
  "data": {
    "handlers": [
      {
        "name": "github::list_labels",
        "description": "List labels in a repository",
        "output_schema": { "type": "array", "definition": { "items": { "type": "string" } } }
      }
    ]
  }
}
```

Each schema level is a `type` tag with its payload under `definition`, and a
variant that carries fields nests one level deeper:

```json
{
  "type": "array",
  "definition": {
    "items": {
      "type": "object",
      "definition": {
        "properties": {
          "name": { "schema": { "type": "string" }, "required": true }
        }
      }
    }
  }
}
```

A property's `required` flag defaults to `false`, and an object schema's
`properties` defaults to empty, so both can be omitted when you do not need
them.

The field is optional and additive, so plugins that omit it still load. An
undeclared handler keeps a `Dynamic` output: that accepts any value but cannot
be structurally validated, so a `for_each` over such a step reports a
dynamic-boundary diagnostic rather than type-checking.

A schema that is not well-formed -- an empty union, for example -- is rejected
when the plugin loads, not midway through a run. Declaring a schema also makes
the host verify the plugin's actual output against it and fail the step on a
mismatch.

Only the output shape can be declared. Strict compilation additionally requires
an input schema and a confidence capability, which the protocol does not yet
carry, so plugin steps remain incomplete contracts under `--strict`.

### Invoke (host -> plugin)

Request:

```json
{
  "method": "Invoke",
  "params": {
    "handler": "github::create_issue",
    "input": { "title": "Bug report", "body": "..." }
  }
}
```

Success response:

```json
{
  "status": "InvokeOk",
  "data": { "output": { "id": 42, "url": "..." } }
}
```

Error response:

```json
{
  "status": "InvokeErr",
  "data": { "error": "authentication failed" }
}
```

### Shutdown (host -> plugin)

Request:

```json
{ "method": "Shutdown" }
```

Response:

```json
{ "status": "ShutdownAck" }
```

## Writing a Plugin

A plugin is any binary that:

1. Reads newline-delimited JSON from stdin
2. Writes newline-delimited JSON to stdout
3. Handles `Declare`, `Invoke`, and `Shutdown` methods

See `crates/crux-plugin/tests/host.rs` and `crates/crux-plugin/tests/bridge.rs`
for working examples.

## Handler Naming

Plugin handlers use `namespace::action` format:

- `github::create_issue`
- `slack::post_message`
- `linear::create_ticket`
- `jira::transition_issue`

The namespace comes from the `name` field in `plugins.toml`.
