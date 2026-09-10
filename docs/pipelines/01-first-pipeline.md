# Your first pipeline

## Install

Build the pipeline runner from source:

```bash
cargo build -p crux-cli --bin crux --release
```

The binary lands at `target/release/crux`. Add it to your PATH or
run it directly.

## Write a pipeline

Create a file called `hello.crux`:

```yaml
pipeline: hello
budget: { steps: 3 }
display:
  title: Hello Pipeline
  output: auto
  steps:
    greet: Greeting
    timestamp: Timestamp

steps:
  - step: greet
    handler: shell::capture
    args:
      cmd: "echo hello from crux"

  - step: timestamp
    handler: shell::capture
    args:
      cmd: "date -u +%Y-%m-%dT%H:%M:%SZ"

```

Every `.crux` file has four parts:

- **`pipeline:`** -- a name for the pipeline
- **`budget:`** -- optional limits measured independently: handler attempts (`steps`),
  model tokens (`tokens`), wall-clock milliseconds (`duration_ms`), and dollars (`usd`)
- **`display:`** -- optional human-facing title, labels, and output visibility
- **`steps:`** -- an ordered list of steps to execute

## Run it

```bash
crux run hello.crux --summary
```

Output looks like:

```text
Hello Pipeline  PASS  42ms

  ✓ Greeting                                       12ms
  ✓ Timestamp                                       8ms

2/2 checks passed
```

The summary shows every step, its status, and wall-clock duration. Successful shell output is
hidden in `auto` mode; semantic pipeline results remain visible.

## Verbosity

```bash
crux run hello.crux           # compact result JSON (compatibility default)
crux run hello.crux --summary # concise human-readable summary
crux run hello.crux -v        # full trace and raw final output
crux run hello.crux --json    # explicitly select compact result JSON
crux run hello.crux -q        # errors only
```

## Passing input

Some pipelines accept JSON input:

```bash
crux run pipeline.crux input.json
```

The input JSON is available to handlers as the initial pipeline state.
Steps that don't need external input (like our `hello.crux`) can run
without it.

## What just happened

The two shell steps ran in order, with each output becoming the next step input.
`--summary` used the display metadata for the title and step labels while hiding the final
shell envelope in `auto` mode. The whole run was traced into a `Crux<T>` value internally --
the same structure you get from `#[crux::agent]` in Rust.

Next: [Handlers](./02-handlers.md) -- what you can do in each step.
