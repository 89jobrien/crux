# Your first pipeline

## Install

Build the pipeline runner from source:

```bash
cargo build -p crux-agentic --bin crux --release
```

The binary lands at `target/release/crux`. Add it to your PATH or
run it directly.

## Write a pipeline

Create a file called `hello.crux`:

```yaml
pipeline: hello
budget: { calls: 3 }

steps:
  - step: greet
    handler: shell::capture
    args:
      cmd: "echo hello from crux"

  - step: timestamp
    handler: shell::capture
    args:
      cmd: "date -u +%Y-%m-%dT%H:%M:%SZ"

  - step: log_output
    handler: ctrl::log
```

Every `.crux` file has three parts:

- **`pipeline:`** -- a name for the pipeline
- **`budget:`** -- limits on how many steps can run (`calls`, `tokens`,
  `duration_ms`, `cost_cents`)
- **`steps:`** -- an ordered list of steps to execute

## Run it

```bash
crux run hello.crux
```

Output looks like:

```text
Pipeline: hello
Status:   OK
Duration: 42.1ms
Steps:    3

Trace:
   1. [  OK] greet (12ms)
   2. [  OK] timestamp (8ms)
   3. [  OK] log_output (0ms)

Output:
{
  "exit_code": 0,
  "stdout": "2026-05-25T12:00:00Z\n",
  "stderr": ""
}
```

The trace shows every step, its status, and wall-clock duration. The
output is the return value of the last step.

## Verbosity

```bash
crux run hello.crux -q    # quiet: status line only
crux run hello.crux -v    # verbose: full step output at each stage
```

## Passing input

Some pipelines accept JSON input:

```bash
crux run pipeline.crux input.json
```

The input JSON is available to handlers as the initial pipeline state.
Steps that don't need external input (like our `hello.crux`) can run
without it.

## Shorthand

A first argument that names an existing file runs as a pipeline, so the
`run` is optional:

```bash
crux hello.crux
```

The same shorthand names a target when the argument isn't a file --
see [Cruxfiles](07-cruxfiles.md).

## What just happened

Each step ran in order. The output of one step becomes the input to
the next. `ctrl::log` printed the current state to stderr and passed
it through unchanged. The whole run was traced into a `Crux<T>` value
internally -- the same structure you get from `#[crux::agent]` in Rust.

Next: [Handlers](./02-handlers.md) -- what you can do in each step.
