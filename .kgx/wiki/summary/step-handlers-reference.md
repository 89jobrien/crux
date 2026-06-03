---
title: Step Handlers Reference
source_document: crux_remaining_crates
tags: [handlers, agentic, reference]
---

# Step Handlers (crux-agentic)

55+ handlers registered via [[HandlerRegistry]] from [[crux-script]].

## llm (3)
- `llm::invoke` -- LLM with provider fallback
- `llm::stream` -- streaming output (stub)
- `llm::invoke_with_fallback` -- tiered: Anthropic > OpenAI > Ollama

## container (2)
- `container::run` -- spawn with resource constraints
- `container::wait` -- poll until completion

## harness (2)
- `harness::evolve` -- propose resource profile evolution
- `harness::canary` -- validate candidate profile

## analysis (9)
latency_profile, token_spend, failure_clusters, replay_cache_hits,
tighten_budget, compress_stages, tune_retry, patch_schema_check,
replay_dry_run

## ci (8)
compile_errors, clippy_violations, nextest_failures, deny_violations,
deduplicate_spans, classify_severity, attach_owners, score_fixability

## review (8)
arch_boundary_check, normalize_findings, apply_severity, compute_score,
approve, detect_antipatterns, group_by_file, compose_daily_note

## triage (19)
parse_repo_tags, score_urgency, deduplicate_intent, group_by_repo,
merge_results, parse_env_probe, classify_severity, suggest_remediation,
correlate_failures, measure_overhead, detect_orphaned_worktrees,
build_cleanup_plan, match_todos_to_issues, identify_untracked,
match_plans_to_commits, detect_status_mismatch, categorize_commits,
classify_true_false, generate_allowlist_entries

## rx (3)
run, list, install

## sqlite (7)
exec, query_many, query_one, insert, update, delete, upsert
