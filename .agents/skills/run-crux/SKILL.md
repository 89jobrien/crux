---
name: run-crux
description: >
  Build and drive the Crux CLI: list pipelines, validate or execute `.crux`
  files, generate plans, or run an uncredentialed smoke test.
---

# Run Crux

The Cargo package is `crux-cli`; it builds binary `target/debug/crux`.

## Uncredentialed smoke

```bash
.agents/skills/run-crux/smoke.sh
```

The script resolves the repository from its own location and changes to that
root before building or running examples. It is safe to invoke from any current
working directory. It builds default features, checks help/list/validation,
executes only `examples/read_and_pick.crux`, and runs the rule planner. It does
not execute BAML or credentialed pipelines.

## Direct commands

Run relative-path examples from the repository root:

```bash
cargo build -p crux-cli
./target/debug/crux --help
./target/debug/crux list examples
./target/debug/crux run examples/read_and_pick.crux --check
./target/debug/crux run examples/read_and_pick.crux examples/input_read_and_pick.json
./target/debug/crux plan --goal "fetch data and summarize it"
```

The rule planner emits templates. Register or replace any generated handler that is not built in
before executing the result with `--strict`.

`run --check` parses and validates without executing handlers. There is no
standalone `crux check` CLI variant. `run -` reads a pipeline or Cruxfile from
stdin. The default rule planner needs no API key; `plan --planner llm` requires
the `baml` feature and provider credentials.

Some checked-in pipelines use relative paths internally, so direct execution
must use repository-root cwd even when the pipeline path itself is absolute.
The smoke script handles this with `cd "$ROOT"`.

## Test

```bash
cargo nextest run -p crux-cli
```

Use `cargo nextest run`, not `cargo test`. Do not use extraction, decomposition,
or LLM-planner examples for uncredentialed smoke checks.
