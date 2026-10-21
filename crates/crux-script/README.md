# crux-script

Parser, validator, typed compiler, registry, and interpreter for YAML `.crux` pipelines. It lets
applications define orchestration without recompiling Rust code.

## Architecture role

The crate owns the declarative execution model. `HandlerRegistry` is the extension boundary;
`crux-stdlib`, `crux-agentic`, `crux-baml`, and `crux-plugin` supply handlers. `Runner` executes a
validated `PipelineDef` against `CruxCtx` and returns a complete `Crux<Value>` trace.

Handlers conform to a standard **contract** (see [`docs/CONFORMANCE.md`](../../docs/CONFORMANCE.md)):
- Accept JSON input (`Value`)
- Return `Result<HandlerOutput, CruxErr>` with optional confidence scores
- Execute asynchronously with timeout, retry, and error-recovery support

For registry architecture and contract-based handler loading, see [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md).

## Pipeline formats

- A pipeline has a top-level `pipeline:` key and one execution flow.
- A Cruxfile has `targets:` and resolves named targets, dependencies, and budgets.
- Expressions interpolate input, variables, iteration bindings, and prior step output/confidence.
- Step forms include handler calls, delegation, pipes, joins, confidence routes, speculation,
  `for_each`, `while`, `repeat`, and polling, with retry/error/expectation controls.

```yaml
pipeline: read-file
steps:
  - step: read
    handler: fs::read
    args:
      path: README.md
```

## Rust usage

```rust
use std::sync::Arc;
use crux_script::{HandlerRegistry, Runner, load};

let pipeline = load("pipeline: noop\nsteps: []\n")?;
let runner = Runner::new(Arc::new(HandlerRegistry::new()));
let trace = runner.run(&pipeline, serde_json::Value::Null).await;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Key API

- `load`, `load_file`, `load_cruxfile`, `is_cruxfile`: parsing entry points.
- `HandlerRegistry`, `StepRunner`, `StepRunnerRegistry`: handler and typed runner registration.
- `Runner`: checked execution, replay execution, unchecked execution, and Cruxfile targets.
- `validate_pipeline`, `validate_cruxfile`: diagnostics without execution.
- `compile_pipeline`, `CompileOptions`, `CompileMode`, `TypedPipeline`: permissive or strict typed
  compilation. Strict mode rejects dynamic boundaries and missing contracts/input schemas.
- Metadata/schema types describe arguments, values, confidence, risk, capabilities, and effects.

## Features and status

There are no Cargo features. The typed compiler currently compiles simple handler-oriented steps and
emits diagnostics for unresolved or dynamic contracts; the interpreter remains the full execution
surface. Pipelines are hand-authored YAML, not generated Rust source.

## Development and testing

```console
cargo nextest run -p crux-script
cargo clippy -p crux-script --all-targets -- -D warnings
cargo fmt --all -- --check
```

Integration tests cover parsing, validation, typed compilation, expressions, retries, errors,
timeouts, loops, budgets, confidence, and runner contracts.
