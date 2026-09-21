# Handoff — crux (2026-09-20)

| ID | P | Status | Title |
|---|---|---|---|
| t14 | P2 | open | Infer pipe and join types |
| uncommitted-work | P1 | open | Uncommitted changes (9 files) |

## Log

- 20260920.012242: done=25 running=1 pending=30 blocked=0 [86aa628, b8c7fe8, dc41728, 81a7940, e6ced60, 798e65b, 82fd28a, de45a1c, 02e0a15, 8b03908]
- 20260920.003720: done=24 running=1 pending=31 blocked=0 [b8c7fe8, dc41728, 81a7940, e6ced60, 798e65b, 82fd28a, de45a1c, 02e0a15, 8b03908, 6d45857]
- 20260909.015838: done=7 running=0 pending=22 blocked=1 [d12599b, dc34c5f, 8797583, 8bed133, fa39f72, 5153af6, 7a49aab, 38ad396, e2bff9c, b325364]
- 20260823.081522: done=2 running=0 pending=12 blocked=0
- 20260822.234500: GitHub issue triage-and-fix pass: closed 24 issues across bug fixes, feature completion,
and stale issue cleanup. Bug fixes: #91 (BudgetTracker bounds), #68/#9 (speculate
tie-break ordering), #75/#76 (confidence validation range), #103 (expr.rs strip
whitespace). Feature completions: full crux-script control-flow batch #79-89, #71 (CLI
JSON output), #70 (json::jq extensions), #69 (BAML function audit), #26 (PlanRule
dedup). Infrastructure: #12 (examples cleanup: joe/ctrl::noop rewiring), plus 10 issues
closed as stale/duplicate/already-resolved (#8,14,15,17,11,10,7,27,13,67). Filed 2
follow-up issues: #104 (crux schema command), #105 (serde-saphyr parser bug).
All 776 tests pass (cargo nextest), 0 clippy warnings. Workspace version 0.3.1.
HEAD: 86408da (merge #12).

