---
crate: crux-script
pattern: layered
layers:
  - name: parse
    module: schema
    input: "YAML string"
    output: "PipelineDef / CruxfileDef"
  - name: validate
    module: validator
    input: "Parsed def"
    output: "ValidationReport"
  - name: resolve
    module: resolve
    input: "Cruxfile + target"
    output: "Single pipeline"
  - name: execute
    module: runner
    input: "Pipeline + input"
    output: "Final output JSON"
  - name: dispatch
    module: step_runner
    input: "Step def + context"
    output: "StepOutput"
  - name: lookup
    module: registry
    input: "Step type string"
    output: "Handler function"
extension_points:
  - HandlerRegistry
  - StepRunner (trait)
  - TargetResolver
---

# Architecture: crux-script

Pipeline execution engine with a layered design:

## Layers

```
.crux file (YAML)
  -> schema (parse)
  -> validator (static checks)
  -> resolve (target resolution for Cruxfiles)
  -> runner (execution orchestration)
  -> step_runner (per-step dispatch)
  -> registry (handler lookup)
```

## Extension Points

- **`HandlerRegistry`** — register custom step handlers by type string
- **`StepRunner` trait** — implement custom step execution logic
- **`TargetResolver`** — resolve Cruxfile targets to pipelines

## Expression Language

`expr` module supports template expressions in pipeline YAML:
`${{ steps.prev.output.field }}` style interpolation with JSON path
traversal.
