---
crate: crux-script
type: engine
description: "YAML-driven pipeline scripting for the crux agentic DSL"
version: "0.3.0"
edition: "2024"
dependencies:
  - serde_saphyr
  - thiserror
file_extension: ".crux"
formats:
  - name: Pipeline
    key: "pipeline:"
    description: "Single execution flow"
  - name: Cruxfile
    key: "targets:"
    description: "Multi-target build file"
modules:
  - name: schema
    purpose: "PipelineDef, CruxfileDef YAML schema types"
  - name: runner
    purpose: "Pipeline execution engine"
  - name: step_runner
    purpose: "Per-step dispatch and StepRunner trait"
  - name: registry
    purpose: "HandlerRegistry for handler registration"
  - name: resolve
    purpose: "TargetResolver for Cruxfile target resolution"
  - name: validator
    purpose: "Static validation of pipelines and Cruxfiles"
  - name: expr
    purpose: "Expression evaluation in pipeline templates"
  - name: metadata
    purpose: "HandlerMetadata, capabilities, risk levels"
  - name: handler_output
    purpose: "HandlerOutput return type"
---

# crux-script

YAML-driven pipeline scripting for the crux agentic DSL. Define agent
pipelines declaratively in `.crux` files (YAML syntax), register step
handlers in Rust, and execute without recompilation.

## Usage

```rust
use crux_script::{load_file, HandlerRegistry, Runner};

let pipeline = load_file("my_pipeline.crux")?;
let mut registry = HandlerRegistry::new();
// register handlers...
let runner = Runner::new(registry);
runner.run(&pipeline, input).await?;
```

## Cruxfile vs Pipeline

- **Pipeline** — single execution flow with `pipeline:` key
- **Cruxfile** — multi-target build file with `targets:` key
