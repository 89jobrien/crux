# Design: README Positioning

## Goal

Clarify Crux's identity as a Rust framework whose `Crux<T>` execution value
keeps a typed result and its inspectable, serializable, replayable execution
history together.

## Approved Approach

Rewrite the README opening around the term "execution value" and add a concise
`Why Crux?` section without naming competing projects.

## Context Map

### Files to Modify

| File | Purpose | Changes Needed |
| --- | --- | --- |
| `README.md` | Project introduction | Rewrite opening; add positioning |

### Dependencies

None. The change documents the existing `Crux<T>` API and does not alter code
or public interfaces.

### Validation

Run the repository's Markdown checks if available. Inspect the rendered
structure and verify referenced API names against the current source.

## Documentation Structure

1. Open with `Crux<T>` as a typed execution value.
2. Explain that result, errors, steps, child runs, identity, and timing remain together.
3. Retain YAML pipelines and typed Rust agents as equal authoring surfaces.
4. Add `Why Crux?` to distinguish execution values from plain results,
   side-channel traces, and external checkpoints.
5. Leave installation, CLI, crate inventory, feature flags, and documentation links unchanged.

## Public API

No public API changes.

## Out of Scope

- Naming or comparing competing projects.
- Changing examples, commands, versions, crate descriptions, or implementation.
- Adding new claims not supported by the existing `Crux<T>` documentation.

## Risk

- [x] Breaking API changes: no
- [x] New external dependency: no
- [x] Feature flag required: no
- [x] Scope limited to documentation positioning
