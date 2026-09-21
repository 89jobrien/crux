# xtask

Workspace task-runner shim and release publisher for Crux. This package is an internal binary
(`publish = false`), not a reusable library.

## Architecture role

`cargo xtask <command>` normally forwards its arguments and exit status to the installed `taskit`
binary. If `taskit` is missing, the shim runs `cargo install taskit` and retries. The one local
command, `publish`, publishes every public workspace package in dependency order and waits for each
version to appear in the crates.io sparse index before continuing.

## Usage and commands

```console
cargo xtask check quick
cargo xtask test run
cargo xtask publish
cargo xtask publish --from crux-runtime
```

All commands except `publish` are defined by the installed `taskit` version; use
`cargo xtask --help` and `cargo xtask <command> --help` for its current interface. `--from` resumes
the fixed publish order at the named package. Publishing is real: there is no dry-run mode in this
shim, and it invokes `cargo publish -p <package>`.

## Implementation status

- The publish list covers all publishable workspace packages and is dependency-ordered by tests.
- Index polling uses 30 attempts at ten-second intervals and stops on the first HTTP error.
- Workspace version parsing reads the first quoted `version` entry in the root `Cargo.toml`.
- The shim has no feature flags and no generated source.

## Development and testing

Run from the workspace root:

```console
cargo nextest run -p xtask
cargo clippy -p xtask --all-targets -- -D warnings
cargo fmt --all -- --check
```

Tests cover argument parsing, sparse-index matching, publish-order completeness, and dependency
ordering. They do not publish crates.
