# Handoff — crux (20260419:210016)

**Branch:** main | **Build:** clean | **Tests:** 329 passed, 0 skipped

## Items

| ID      | P   | Status | Title                                               |
| ------- | --- | ------ | --------------------------------------------------- |
| crux-26 | P2  | open   | Implement crux-planner: goal-to-pipeline generation |

## Log

- 20260419:210016: Audited crux-script pipeline capabilities. Created docs/crux-capabilities.md
  (handler/combinator support matrix). Filed GH issues #7-#12 for 6 known gaps. Added pipeline
  capabilities reference to CLAUDE.md. 329/329 tests pass, build clean.
- 20260419::230000: handjobs triage — 1 open HANDOFF item, 7 GH issues synced (6 new gap issues
  #7-#12 created from docs/crux-capabilities.md audit). No local TODOs found.
- 20260420::001527: Fixed CI fmt failure (baml_client stub), reordered gates, added nightly release
  workflow, env-configurable BAML models with fallback defaults. [7d5d47e, ca00d9b, 8143fda]
- 20260419::193106: Implemented cruxai-model crate, wired into crux-agentic adapters, council fixes
  (MissingSegment, Mistral tiers). 76/76 tests pass. [8ba0dca..9814b63]
- 20260419:165500: Verified minibox-agent (crux-25) complete, committed unstaged files. [08ba4c4]
