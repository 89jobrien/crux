---
title: Pipeline Scripting
source_document: crux_remaining_crates
tags: [script, pipeline, yaml]
---

# Pipeline Scripting (crux-script)

## Pipeline Definition
- [[PipelineDef]] -- name, budget, steps
- [[StepDef]] -- Step / Delegate / Pipe / JoinAll / RouteOnConfidence / Speculate
- [[BudgetDef]] -- token/call/duration/cost limits (struct with Options)
- Pipeline files use `.crux` extension (YAML syntax)
- [[CruxfileDef]] -- multi-target manifest

## Execution
- [[Runner]] wraps [[HandlerRegistry]]
- `run(pipeline, input)` -- execute with validation
- `run_with_replay(pipeline, input, previous, mode)` -- replay support
- `run_unchecked(pipeline, input)` -- skip validation
- `run_target(target, name, budget)` -- single target from Cruxfile

## Handler Registration
- [[HandlerRegistry]] -- maps names to async handlers
- `handler(name, f)` -- register returning [[HandlerOutput]]
- `handler_value(name, f)` -- register returning plain Value
- `agent<A>(name)` -- register crux [[Agent]] for delegation
- [[HandlerOutput]] -- value + optional confidence [0.0, 1.0]
- [[HandlerMetadata]] -- name, description, risk, capabilities

## Validation
- `validate_pipeline()` -- static checks
- `validate_cruxfile()` -- multi-target validation

## Plugins
[[PluginHost]] from [[crux-plugin]] manages subprocess plugins
communicating via JSON-RPC over stdin/stdout. [[PluginDiscovery]] trait
with [[TomlFileDiscovery]] adapter loads from `~/.crux/plugins.toml`.
