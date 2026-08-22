# Blocked: opavs phase gate

All implementation work for issue #69 is complete and verified (see summary below),
but the commit step is blocked by the opavs guardian hook:

```
opavs (/Users/joe/dev/crux): repo is in the ACT phase. git commit/push are only
allowed in SHIP -- run `bash $CLAUDE_PLUGIN_ROOT/scripts/opavs-phase.sh set SHIP`
(in /Users/joe/dev/crux) once the user has actually approved that transition.
```

The task brief stated the phase was "already set to SHIP", but the actual state
file (`/Users/joe/dev/crux/.ctx/.opavs-phase`) reads `ACT`, not `SHIP`. Per the
project's safety rules, only the user can authorize that phase transition — an
agent's own task description is not equivalent to user approval. I did not flip
the phase or bypass the gate.

## Work completed (uncommitted, staged in working tree)

Investigation first: the issue's premise ("only 3 BAML functions wired") was
already stale. `crates/crux-baml/src/extract.rs` already had **9** functions
wired (`ExtractEntities`, `Summarize`, `Classify`, `DescribeProject`,
`AssessHealth`, `ClassifyProject`, `GenerateChangelog`, `SuggestRelated`,
`ClassifyCIFailure`), all defined in `crates/crux-baml/baml_src/extract.baml`,
with mock responses for every one already present in
`crates/crux-baml/tests/mock_baml.rs`. The real gap was: a stale `TODO(#69)`
comment, stale docs, and missing tests for 5 of the 9 wired functions.

Changes made:

1. `crates/crux-baml/src/extract.rs` -- removed the stale
   `// TODO(#69): only 3 BAML functions wired` comment (inaccurate).
2. `crates/crux-baml/tests/llm_extract.rs` -- added 5 new tests following the
   existing mock-server pattern: `describe_project_returns_structured_output`,
   `assess_health_returns_structured_output`,
   `classify_project_returns_structured_output`,
   `generate_changelog_returns_structured_output`,
   `suggest_related_returns_structured_output`.
3. `docs/crux-capabilities.md` -- updated the `llm::extract` row to list all 9
   wired functions, and removed the now-resolved "Only 3 BAML functions wired"
   entry from the Known Gaps table.

No new `.baml` function definitions were added (all needed functions already
existed), so no `baml_client` regeneration was required for the commit (it was
regenerated locally only to compile/run tests -- it's gitignored).

## Verification

- `cargo nextest run -p crux-baml` (with `mise exec -- baml-cli generate` run
  first to produce the gitignored client): **23/23 passed**, including all 5
  new tests.
- `cargo nextest run --workspace --exclude crux-baml`: **767/767 passed**.
- `cargo clippy --workspace --exclude crux-baml --all-targets -- -D warnings`:
  clean.
- `cargo clippy -p crux-baml --all-targets -- -D warnings`: clean.

## Next step

Once the user explicitly approves shipping, run in `/Users/joe/dev/crux`:

```
bash $CLAUDE_PLUGIN_ROOT/scripts/opavs-phase.sh set SHIP
```

then, in the worktree:

```
git -C /Users/joe/dev/crux/.worktrees/issue-69 add -A
git -C /Users/joe/dev/crux/.worktrees/issue-69 commit -m "feat(crux-baml): wire additional BAML functions for llm::extract, fixes #69"
```
