---
crate: crux-script
role: pipeline-engine
key_entry_points:
  - path: "src/schema.rs"
    purpose: "PipelineDef and CruxfileDef YAML types"
  - path: "src/runner.rs"
    purpose: "Pipeline execution loop"
  - path: "src/registry.rs"
    purpose: "HandlerRegistry for handler registration"
  - path: "src/validator.rs"
    purpose: "Static validation before execution"
howto:
  - task: "Add a new step type"
    steps:
      - "Define handler in crux-agentic or crux-stdlib"
      - "Register in the appropriate register_all() function"
      - "Add HandlerMetadata with capabilities and risk level"
      - "Add validation rules in validator.rs if needed"
file_formats:
  - extension: ".crux"
    syntax: YAML
    variants:
      - name: Pipeline
        key: "pipeline:"
      - name: Cruxfile
        key: "targets:"
expression_syntax: "${{ steps.<name>.output.<path> }}"
---

# Agents: crux-script

## For AI Agents Working With This Crate

Pipeline execution engine. Handlers are registered here but implemented
in `crux-agentic`, `crux-stdlib`, and `crux-baml`.

### Pipeline File Format

Files use `.crux` extension (YAML syntax). Two formats:
- **Pipeline** — `pipeline:` top-level key
- **Cruxfile** — `targets:` top-level key (multi-target)

### Expression Syntax

`${{ steps.<name>.output.<path> }}` — JSON path interpolation
resolved at runtime by `expr.rs`.
