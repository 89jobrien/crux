# crux-cli

Command-line interface for discovering, validating, planning, executing, replaying, and persisting
Crux pipeline traces. The package installs the `crux` binary.

## Architecture role

The CLI composes `crux-script` with standard, agentic, and plugin handlers. It is an application
boundary rather than a library crate: command parsing and output modes live under `src/bin/crux/`.

## Commands

```console
crux list [ROOT]
crux run [PIPELINE] [TARGET_OR_INPUT]
crux plan --goal "Read a file and summarize it"
```

`list` recursively discovers `.crux` files. `run` accepts a pipeline, a multi-target Cruxfile, `-`
for stdin, or discovers a Cruxfile in the current directory when no path is given. `plan` writes a
generated pipeline to stdout unless `--output` is supplied.

## Run options

- `--check`, `--dry-run`, and `--strict` validate or inspect without normal execution.
- `--target`, `--input`, and the second positional disambiguate Cruxfile targets and JSON input.
- `--quiet`, `--summary`, `--json`, and `--verbose` are mutually exclusive output modes.
- `--replay <TRACE>` with `--replay-mode strict|lenient` reuses matching prior steps.
- `--save-trace <PATH>` overrides automatic trace persistence.
- `--plugins <PATH>` overrides the default `$HOME/.crux/plugins.toml` manifest.

## Plan options

`--planner rule` is local and default. `--planner llm` requires the `baml` feature and provider
configuration. Output types are `yaml`, `json`, `pretty`, `dry-run`, and `handoff`; constraints are
passed only to the LLM planner.

```console
crux run examples/showcase.crux --check
crux run Cruxfile build --verbose --save-trace .crux/traces/build.json
crux plan --goal "Review a git diff" --output-type pretty
```

## Features and implementation status

| Feature | Default | Effect                                                 |
| ------- | ------- | ------------------------------------------------------ |
| `baml`  | no      | Enables the BAML planner and structured BAML handlers. |

The CLI has no public library API and no generated source. Rule planning works without credentials;
LLM planning and provider handlers require external configuration. Non-strict execution can
inject stubs for unregistered handlers, while `--strict` rejects them. Trace paths for Cruxfile
targets gain a target suffix.

## Development and testing

```console
cargo nextest run -p crux-cli
cargo nextest run -p crux-cli --features baml
cargo clippy -p crux-cli --all-targets --all-features -- -D warnings
./target/debug/crux --help
./target/debug/crux run --help
./target/debug/crux plan --help
```

Tests cover argument conflicts, planning output, delegated Cruxfile targets, trace persistence,
review regressions, plugin planning, and strict validation behavior.
