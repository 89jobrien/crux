# BLOCKED: commit for issue #70

## Feature work status: COMPLETE

- `crates/crux-stdlib/src/json.rs` — `json::jq` extended with:
  - Array indexing on key segments (`.foo[2]`, `.matrix[1][0]`) via a new
    `parse_path_segments`/`PathSeg` tokenizer feeding `traverse`.
  - Pipe composition (`|`) — pre-existing, unchanged.
  - `select(<cond>)` — filters an array by a per-element condition
    (`==`, `!=`, `>`, `<`, `>=`, `<=`, or bare truthy dot-path test).
    Non-array input: passes the value through if the condition holds,
    else returns `null` (single-value analogue of jq's stream `empty`).
  - `map(<expr>)` — applies a sub-expression to each element of an array,
    returns an array of results. Errors on non-array input.
  - Existing dot-path, `keys`, `length`, `type`, `first`, `last`, `has()`
    behavior is unchanged (regression-tested).
- `crates/crux-stdlib/tests/json_handlers.rs` — added tests for bracket
  indexing, nested indexing, `select()` with comparison/equality/truthy,
  `map()`, `map()` error on non-array, and combined
  `select | map` / `map | select` pipelines. Updated the two tests that
  previously asserted `select`/`map` were unsupported (they now assert
  `reduce` is still unsupported, since `reduce`/`foreach`/variable
  bindings/etc. remain out of scope).
- `docs/crux-capabilities.md` — updated the `json::jq` row and the
  "Known Gaps" entry (#70) to describe the new capabilities and the
  remaining honest limitations (no `reduce`, `foreach`, `as $x` bindings,
  array/object construction literals, or stream semantics).
- `crates/crux-stdlib/TODO.md` — marked #70 done.

Verification run and passing:
- `cargo nextest run --workspace --exclude crux-baml --manifest-path .../Cargo.toml -p crux-stdlib` → 775/775 passed.
- `cargo nextest run --workspace --exclude crux-baml --manifest-path .../Cargo.toml` → 775/775 passed.
- `cargo clippy --workspace --exclude crux-baml --all-targets --manifest-path .../Cargo.toml -- -D warnings` → clean, no warnings.

## What's blocking the commit

The repo's pre-commit hook (`.githooks/pre-commit` → `cargo xtask check
pre-commit` → delegates to the external `taskit` binary) runs
`cargo fmt --check --all` across the **entire workspace**, including
`crux-baml`, which is **excluded** from this issue's verification scope
per the task instructions (`--exclude crux-baml` in every test/clippy
command given).

`crux-baml/src/lib.rs` references `mod baml_client;`, but `baml_client/`
is a gitignored, generated directory (see `CLAUDE.md`: "`baml_client/` is
gitignored (generated). Run `mise exec -- baml-cli generate` after
cloning or bumping the baml version."). It does not exist in this
worktree, so `cargo fmt --check --all` fails with:

```
Error writing files: failed to resolve mod `baml_client`:
.../crates/crux-baml/src/baml_client.rs does not exist
```

This is **pre-existing and unrelated to this change** — confirmed by
stashing all edits and re-running `cargo fmt --check --all` directly
against the worktree's baseline commit (`a5b4cc9`), which fails
identically.

### Root-cause attempt

Tried the documented fix (`mise exec -- baml-cli generate` from
`crates/crux-agentic/`), which is blocked by a separate pre-existing
issue: the installed `baml-cli` is `0.221.0` but `generators.baml`
declares `0.222.0`, and BAML refuses to generate on a minor-version
mismatch:

```
Version mismatch: BAML GENERATION DISABLED: Generator version (0.222.0)
!== the installed baml package version (0.221.0).
```

Fixing that would mean bumping the `baml` crate dependency and/or the
mise-managed `baml-cli` toolchain — out of scope for issue #70 and not
something this task's instructions authorized touching.

### Attempts made (3)

1. Ran the commit as instructed; hook failed on `cargo fmt --check --all`
   over `crux-baml`.
2. Verified via `git stash` + re-run that the failure is pre-existing on
   the branch's base commit, not introduced by this change.
3. Attempted the documented root-cause fix (`baml-cli generate`); blocked
   by an unrelated baml-cli/generator version mismatch already present in
   the environment.

## Not done

Per instructions, `--no-verify` was not used. The four modified files are
staged and ready to commit
(`crates/crux-stdlib/TODO.md`, `crates/crux-stdlib/src/json.rs`,
`crates/crux-stdlib/tests/json_handlers.rs`, `docs/crux-capabilities.md`)
but no commit has been created. This needs either:

- a workspace-level fix to generate/vendor `crux-baml`'s `baml_client`
  (bump `baml-cli` to 0.222.0, or pin `generators.baml`/`Cargo.toml` back
  to 0.221.0), or
- scoping the pre-commit hook's `fmt --check` to exclude `crux-baml` when
  its generated client is absent, or
- explicit human authorization to bypass the hook for this commit.
