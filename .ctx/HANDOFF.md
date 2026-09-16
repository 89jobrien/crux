# Handoff — crux (2026-09-09)

| ID | P | Status | Title |
|---|---|---|---|
| t6 | P2 | blocked | Adopt Smart Output in Bamlish CI |
| uncommitted-work | P1 | open | Uncommitted changes (39 files) |

## Log

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

- 20260708:000000: Heavy refactor session across crux runtime and stdlib components:
IOSP decomposition — extracted magic numbers into named constants (crux-agentic).
Runtime refactoring — decomposed long context methods: join_all, step_stream,
step_retryable, step_inner into separate implementations (crux-runtime).
Script dispatch — extracted per-step-kind dispatch logic in execute_step()
(crux-script). Stdlib parsing — decomposed parse_diff() into hunk/line/context
helpers (crux-stdlib). Pure logic separation — separated pure logic from I/O
in run dispatch, eval_jq, and TargetResolver. Dead code removal across crates.
Two branch merges: refactor/iosp-magic-numbers-splits (const extraction) and
fix/dead-code-srp-crux-model (dead code cleanup). All 538+ tests pass, clippy
clean. Scope spans crux-runtime, crux-script, crux-stdlib, crux-agentic,
crux-model, crux-core crates.

- 20260701:120000: Major crux-task implementation session — scaffolded crate with types (TaskError,
TaskState, TaskSnapshot), error handling (anyhow → miette diagnostic errors),
TaskManager core (add_task, get_task, list_tasks, update_task, get_stats).
Implemented dependency management with cycle detection (detect_cycle, topological
sort). SqliteBackend with 6 migrations (tasks table, labels, dependencies, indexes).
Added conformance test suite for RegistryBackend with 32 test cases covering CRUD
operations, state transitions, invariants. Implemented CLI binary (crux-task)
with subcommands: list, show, add, update, delete, deps, plan. Added crux-types
wire types: Priority enum, TaskLabel, DependencyKind, TaskSnapshot (serde all).
Updated crux-baml MockLLM tests with miette error handling. Merged event-bus
feature (crux-agentic event broadcasting). Added CONFORMANCE.md spec. All 538+
tests pass, clippy clean.
