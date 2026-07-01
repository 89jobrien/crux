---
crate: crux-script
test_strategy: mixed
inline_test_modules: 6
dedicated_test_files: 5
test_areas:
  - module: schema
    coverage: "YAML parsing of pipelines and Cruxfiles"
  - module: validator
    coverage: "Static validation, diagnostic reporting"
  - module: runner
    coverage: "End-to-end pipeline execution"
  - module: step_runner
    coverage: "Step dispatch and output handling"
  - module: expr
    coverage: "Template expression evaluation"
  - module: resolve
    coverage: "Target resolution for Cruxfiles"
  - module: registry
    coverage: "Handler registration and lookup"
commands:
  default: "cargo nextest run -p crux-script"
---

# Testing: crux-script

## Test Strategy

6 inline test modules + 5 dedicated test files covering parsing,
validation, execution, and expression evaluation.

## Running

```bash
cargo nextest run -p crux-script
```

## Test Fixtures

Pipeline YAML fixtures are inline in test functions or in `examples/`
at the workspace root.
