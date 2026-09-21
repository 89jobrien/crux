# crux-planner

Goal-to-pipeline planning and metrics-driven harness evolution. It offers a deterministic local path
and an optional BAML-backed LLM path.

## Architecture role

The planner produces YAML for `crux-script`; it does not execute pipelines. `EvolutionPlanner`
separately analyzes run metrics and proposes `HarnessDiff` values consumed by runtime safety and
approval flows.

## Deterministic usage

```rust
use crux_planner::{DeterministicPlanner, PlannerConfig};

let planner = DeterministicPlanner::new(PlannerConfig::default());
let yaml = planner.plan("Read a file and extract entities")?;
assert!(yaml.contains("fs::read"));
# Ok::<(), crux_planner::PlannerError>(())
```

Built-in rules match the first goal keyword among Git, extract, summarize, read, write, and JSON;
otherwise they emit a shell-capture fallback. Output is stable for the same input and config.

## Key API

- `Goal`, `Intent`, and `PlannerError`: planning domain records.
- `DeterministicPlanner` and `PlannerConfig`: fixed keyword composition to YAML.
- `PlanRule` and `RulePlanner`: reusable rule vocabulary from `crux-types`.
- `PipelineGenerator`, `InMemoryGenerator`, and `LlmPlannerGeneric<G>`: injectable generation port.
- `LlmPlanner`: BAML-backed async planner when enabled.
- `EvolutionPlanner` and `RunMetrics`: deterministic memory/timeout recommendations from run data.

## Features

| Feature | Default | Effect |
| --- | --- | --- |
| `baml` | no | Enables `LlmPlanner` through `crux-agentic` with its `baml` feature. |

## Implementation status

The deterministic planner renders syntactically simple YAML but does not itself validate handler
availability. Some emitted names (for example `json::write` and `json::parse`) are planning
vocabulary not currently registered by `crux-stdlib`; consumers must validate generated output.
The LLM path requires provider credentials. Evolution currently proposes memory increases for OOM
or pressure and timeout increases for slow runs; it does not apply diffs.

## Development and testing

```console
cargo nextest run -p crux-planner
cargo nextest run -p crux-planner --features baml
cargo clippy -p crux-planner --all-targets --all-features -- -D warnings
```

Deterministic tests assert stable handler selection and YAML shape. Snapshot/BAML tests may require
generated clients and provider configuration.
