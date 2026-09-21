# crux-stdlib

Standard handler library for local, non-LLM pipeline work: filesystem access, Git inspection, JSON
transforms, text parsers, shell execution, and small control operations.

## Architecture role

This crate implements handlers for `crux-script::HandlerRegistry`; it does not own parsing or the
runtime. Handler metadata describes arguments, capabilities, risk, side effects, confidence, and
whether an operation is deterministic. `crux-agentic::register_all` installs this library first.

```rust
use crux_script::HandlerRegistry;

let mut registry = HandlerRegistry::new();
crux_stdlib::register_all(&mut registry);
```

## Handlers

| Family | Registered names |
| --- | --- |
| Filesystem | `fs::read`, `fs::write`, `fs::glob`, `fs::exists` |
| Git | `git::staged_files`, `git::diff`, `git::log`, `git::status` |
| JSON | `json::pick`, `json::merge`, `json::group_by`, `json::filter_nonempty`, `json::jq` |
| Text | `text::parse_vimgrep`, `text::parse_jsonl`, `text::parse_frontmatter` |
| Text (continued) | `text::parse_diff`, `text::parse_branch_list` |
| Shell | `shell::exec`, `shell::capture` |
| Control | `ctrl::noop`, `ctrl::log`, `ctrl::assert` |

The public text parser functions can also be called directly. `StdlibError`, `require_str`, and
`opt_str` support custom handlers that follow the same input conventions.

## Example pipeline

```yaml
pipeline: inspect-status
steps:
  - step: status
    handler: git::status
```

## Features and implementation status

There are no Cargo features and no generated code. "Standard library" does not mean every handler
is pure: filesystem, Git, and shell handlers depend on external state and metadata marks those
effects. There are no direct LLM or HTTP clients; shell commands can still perform arbitrary effects
chosen by the pipeline author and should be governed accordingly.

## Development and testing

```console
cargo nextest run -p crux-stdlib
cargo clippy -p crux-stdlib --all-targets -- -D warnings
```

Unit and integration tests use temporary directories and cover filesystem behavior, Git output,
shell capture, control assertions, JSON transforms, and text parser fixtures.
