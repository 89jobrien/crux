# Handoff — crux (20260419::142500)

**Branch:** main | **Build:** ok | **Tests:** 241/241 passed

## Items

| ID      | P  | Status | Title                                                             |
|---------|----|--------|-------------------------------------------------------------------|
| crux-25 | P1 | open   | minibox-agent: FallbackChainAdapter implementing crux LlmProvider |
| crux-26 | P2 | open   | Implement crux-planner: goal-to-pipeline generation               |

## Log

- 20260419: Added llm::decompose handler + decompose_spec pipeline example. 241/241 tests pass. [87764f3, 397e422]
- 20260418: LlmProvider trait + LlmStep + typed adapters. crux-agentic 0.2.3 published. [129e3bd, a4ef7de]
- 20260418: Gitignored baml_client/, CLAUDE.md BAML section, timestamp format. [4cc0c50, b9bc089]
- 20260417: Fixed BAML version mismatch 0.218→0.221, check-baml recipe. [2ce0e0f, c618719]
- 20260417: Pipeline examples (extract_entities, extract_summary), planner design spec. [df3ae65]
