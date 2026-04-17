# Handoff — crux (2026-04-17)

**Branch:** main | **Build:** ok | **Tests:** 226 passed, 0 skipped

All items complete. cruxai v0.2.1 published to crates.io. crux-agentic handler suite integrated.

## Items

| ID | P | Status | Title |
|---|---|---|---|
| crux-18 | P1 | done | Rename crates crux-* -> cruxai-* for crates.io publish |
| crux-17 | P2 | done | Publish-readiness: docs, examples, CHANGELOG, cargo-deny |
| crux-1 | P0 | done | Phase 2: Lifecycle hooks and replay in CruxCtx |
| crux-2 | P0 | done | Phase 3: End-to-end proc macro testing |
| crux-3 | P1 | done | Phase 4: DelegationBuilder and Speculation |
| crux-4 | P1 | done | Phase 5-6: Replay engine and TaskRegistry |
| crux-7 | P1 | done | SOLID refactoring: decompose CruxCtx into collaborators |
| crux-8 | P1 | done | GitHub Actions CI, git hooks, and justfile |
| crux-11 | P1 | done | Normalize join_all budget check and failure hooks |
| crux-12 | P1 | done | Tighten ReplayMode::Lenient identity checks in forward scans |
| crux-13 | P1 | done | Wire proc macro attributes: replay and registry |
| crux-15 | P1 | done | Streaming steps with incremental events |
| crux-16 | P1 | done | Mid-run checkpoint and resume from TaskRegistry |
| crux-10 | P1 | done | Replace rusqlite with redb (pure-Rust embedded KV) |
| crux-5 | P2 | done | Phase 7: Persistent storage adapter |
| crux-6 | P2 | done | CLAUDE.md and Justfile |
| crux-9 | P2 | done | route_on_confidence, pipe operator, join_all |
| crux-14 | P2 | done | Wire tracing feature flag with conditional instrumentation |

## Log

- 2026-04-17: Session 10: Completed crux-agentic with 17 handlers (ctrl, shell, fs, git, json, llm).
  ArmDef schema upgraded. Static args injection. crux-run migrated to crux-agentic. joe/ examples
  rewritten. crux-19 closed. Workspace v0.2.1, serde-saphyr swap. 226/226 tests. [0fc2808]
- 2026-04-13: Session 7: Completed crux-17 publish-readiness. Added crux-script crate and 7 YAML
  pipeline examples. Renamed all four crates to cruxai-* (crux-18). Published cruxai v0.1.0.
  196/196 tests passing. [1dd57d1, c50a74a, c888375, 21914aa]
- 2026-04-13: Moved HANDOFF.yaml to .ctx/, added readme fields to sub-crates, reorganized docs into
  walkthrough/, renamed crux→cruxai throughout. [1abca9c, 56e6760, 9532bf1, 50a62a0]
- 2026-04-12: Session 6: Wired replay/registry macro attrs, tracing instrumentation, step_stream(),
  checkpoint/resume. 180 -> 196 tests. [0bfadd0]
- 2026-04-12: Session 5: Tightened lenient replay identity with content_hash. 173+7=180 tests.
- 2026-04-12: Session 4: Replaced rusqlite with redb. Normalized join_all hooks. 173 tests.
