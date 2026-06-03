---
title: README v2 Rewrite
source_document: readme_v2
tags: [summary, readme, documentation]
---

# README v2 Rewrite

**Source:** README.md (2026-05-25)

## Key Changes from v1

- Reframed pitch: YAML pipelines first, Rust as escape hatch
- Single compact example showing both .crux file and Rust equivalent
- Moved handler tables, pipeline output samples, orchestrator patterns to docs/
- Reduced from ~307 lines to ~97 lines
- Fixed version (0.1 -> 0.2), dead method reference (steps() -> delegations()), dead link (docs/reference.md)

## Entities

- [[crux]] (project) -- agentic workflows as YAML pipelines
- [[.crux file]] (artifact) -- YAML pipeline definition format
- [[Crux<T>]] (type) -- typed execution trace
- [[CruxCtx]] (type) -- runtime context, injected as x by macro
- [[#[crux::agent]]] (macro) -- annotates async fn for trace generation

## Crate Map

- [[crux-runtime]], [[crux-types]], [[crux-macros]], [[crux-agentic]]
- [[crux-script]], [[crux-model]], [[crux-plugin]], [[crux-planner]]
