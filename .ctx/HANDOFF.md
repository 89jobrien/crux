# Handoff — cruxx (20260422:130205)

**Branch:** main | **Build:** unknown (fork exhaustion s29) | **Tests:** 18/18 bats + 360/360 Rust (s28)

## Items

No open items.

## Log

- 20260422:130205: Generalized .githooks/pre-commit and pre-push (removed Maestro hardcoding,
  auto-detect crates via cargo metadata, env-var config for shared crates/conformance/TF dir).
  Added 18-test bats suite + fuzz.sh (filename classifier + git-ref fuzzer). All 18 pass.
  [fd35e13, f4a66c7, e4af825, e4c1f4d, a300557, d4a7505, 4230e53, 6f5b835, c11892b]
- 20260421:052733: Structured BAML types for GeneratePipeline planner; SafetyPolicy +
  ApprovalGate conformance tests (45 total); fixed 7 example pipeline audit issues. [482936e, 262dbce, 390a2ed]
- 20260420:173838: Closed all 3 HANDOFF items — DockerContainerClient (cruxx-28), LlmPlanner
  via BAML (cruxx-26), stale macro comments (cruxx-27 already fixed). 360/360 tests. [bce2cbb, 68164ee]
- 20260420:132031: Walkthrough series revised (8 docs); CLAUDE.md updated for new crates and
  macros. 358/358 tests. [438d7d5, afc798c]
- 20260420:131613: Orchestrator patterns implemented (HarnessProfile, SafetyPolicy,
  ApprovalGate, EvolutionPlanner, harness/evolve macros, container handlers). 358 tests. [115846b..3772d0b]
