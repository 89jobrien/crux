# Built-in handler catalog

This follows `crux_agentic::register_all`, which registers `crux_stdlib` first.
Pipeline `args` arrive under input key `args`; “input” below means data inherited
from the prior step. Rows marked **score** use `handler` and emit confidence;
other rows use `handler_value` and emit no confidence.

## Standard library

| Handler | Required and optional shape | Output |
| --- | --- | --- |
| `shell::exec` | args `cmd`; optional `cwd`, `env`, `ignore_exit` | `{exit_code,stdout,stderr}`; nonzero allowed |
| `shell::capture` | same | same; nonzero fails unless `ignore_exit` |
| `fs::read` | args `path` | `{content,path}` |
| `fs::write` | args `path`, `content` | `{written:true,path}` |
| `fs::glob` | args `pattern` | `{paths}` |
| `fs::exists` | args `path` | `{exists,path}` |
| `git::staged_files` | optional args `cwd` | `{files}` |
| `git::diff` | optional args `cwd`, `revision` | `{diff}` |
| `git::log` | optional args `cwd`, `n` (default 10) | `{commits:[{hash,subject}]}` |
| `git::status` | optional args `cwd` | `{porcelain,clean}` |
| `json::pick` | input object; optional args `fields` | selected top-level fields |
| `json::merge` | input object; optional args `with` | merged object; overlay wins |
| `json::group_by` | input `items`, `findings`, or `todos`; optional args `key` | map of groups to arrays |
| `json::filter_nonempty` | input `items` or `results`; optional args `field` | `{items}` |
| `json::jq` | args `expr` | evaluated JSON; limited jq subset, not jq CLI |
| `text::parse_vimgrep` | input `text` or `output` | `{matches:[{file,line,col,text}]}` |
| `text::parse_jsonl` | input `text` or `output` | `{items}`; invalid lines skipped |
| `text::parse_frontmatter` | input `text` or `output` | `{frontmatter,body}` |
| `text::parse_diff` | input `text` or `output` | `{files}` |
| `text::parse_branch_list` | input `text` or `output` | `{branches:[{name,current}]}` |
| `ctrl::noop` | any | unchanged input |
| `ctrl::log` | optional args `field`, `compact`, `pretty` | logs stderr; unchanged input |
| `ctrl::assert` | optional args `condition`, `message` | unchanged input or failure |

## LLM, container, harness, and rx

| Handler | Shape | Output and caveat |
| --- | --- | --- |
| `llm::invoke` | args `prompt`; optional `system`, `max_tokens`, `schema`, `client` | `{content,confidence,attempts,client,schema_valid}` plus any `schema` fields; BAML-routed; emits confidence |
| `llm::stream` | same | invoke output plus `streaming:true` and `chunks`; BAML-routed |
| `llm::invoke_with_fallback` | same (legacy `tiers` accepted, ignored) | invoke output; pins BAML's `Auto` fallback client |
| `container::run` | optional args `image`, `cmd`, `memory_mb`, `cpu_millicores`, `timeout_seconds` | `{container_id,state}`; mock unless `docker` |
| `container::wait` | args `container_id`; optional `timeout_seconds` | `{state}` |
| `harness::evolve` | args `base_profile` | `{proposed_profile,diff}`; fixed +256 MiB, not planner execution |
| `harness::canary` | args `candidate_profile` | fixed 15% `Promoted`; no deployment |
| `rx::run` | args `name`; optional `args`, `registry` | `{exit_code,stdout,stderr}` |
| `rx::list` | optional args `registry` | `{commands:[{name,runtime,source,install_path,description}]}` |
| `rx::install` | args `name`, `source`; optional `registry`, `runtime`, `description` | `{installed,path}`; copies/registers script |

## Analysis

| Handler | Input | Output |
| --- | --- | --- |
| `analysis::latency_profile` | `steps` with `started_at`, `completed_at` | `{slow_steps,median_ms}` |
| `analysis::token_spend` | `steps[*].output.metadata.tokens` | `{by_step,total,top3}` |
| `analysis::failure_clusters` | failed `steps` | `{clusters}` |
| `analysis::replay_cache_hits` | `steps[*].cache_hit` | `{by_step}` |
| `analysis::tighten_budget` **score** | `token_spend.total`, `budget.tokens` | suggestion or ratio plus confidence |
| `analysis::compress_stages` | `token_spend.by_step`, `token_spend.total` | `{suggestions}` |
| `analysis::tune_retry` | `failure_clusters.clusters` | `{suggestions}` |
| `analysis::patch_schema_check` | `patch` YAML text | `{valid,errors}`; syntax only |
| `analysis::replay_dry_run` | `trace_path`, `patch` | Currently unusable: invokes external `crux replay`, but the CLI has no `replay` subcommand |

## CI

| Handler | Input | Output |
| --- | --- | --- |
| `ci::compile_errors` | `log` | `{errors}` |
| `ci::clippy_violations` | `log` | `{violations}` |
| `ci::nextest_failures` | `log` | `{failures}` |
| `ci::deny_violations` | `log` or `stdout` | `{violations}` |
| `ci::deduplicate_spans` | `errors`, `violations`, `failures` | object with deduplicated present collections |
| `ci::classify_severity` | `items` with `source` | `{ranked}` |
| `ci::attach_owners` | `ranked` | `{ranked}` with `crate_name`; runs `cargo metadata` in cwd |
| `ci::score_fixability` **score** | `ranked` | `{ranked}` plus confidence |

## Review

| Handler | Input | Output |
| --- | --- | --- |
| `review::arch_boundary_check` | `files` | `{violations}`; invokes `rg` |
| `review::normalize_findings` | `clippy`, `arch`, `coverage` | `{findings}` |
| `review::apply_severity` | `findings` | `{findings}` with tier |
| `review::compute_score` **score** | `findings` | `{score,blocking_count,total_findings?}` |
| `review::approve` | optional args or input `pr` | `{approved:true}`; runs `gh pr review --approve` |
| `review::detect_antipatterns` | parsed-diff `files` | `{findings}` |
| `review::group_by_file` | `findings` | map of file to findings |
| `review::compose_daily_note` | category arrays such as `feat`, `fix` | `{content,date}` |

## Triage

| Handler | Input | Output |
| --- | --- | --- |
| `triage::parse_repo_tags` | `todos` | `{todos}` with repo |
| `triage::score_urgency` | `todos` | sorted `{todos}` with urgency |
| `triage::deduplicate_intent` | `todos` | `{groups}` |
| `triage::group_by_repo` | `todos` | `{repos}` |
| `triage::merge_results` | named gate objects | `{passed,failed,gates}` |
| `triage::parse_env_probe` | probe output objects | `{findings}` |
| `triage::classify_severity` **score** | `findings` | findings/counts plus health confidence |
| `triage::suggest_remediation` | `broken`, `unloaded`, `missing` | `{fixes}` |
| `triage::correlate_failures` | `items`, `recent_failures.output` | `{correlated_failures,total_hooks}` |
| `triage::measure_overhead` **score** | duration `items` | `{p50_ms,p95_ms,sample_count}` plus confidence |
| `triage::detect_orphaned_worktrees` | `worktree_list.output` | `{orphaned_worktrees}` |
| `triage::build_cleanup_plan` **score** | `branches`, `orphaned_worktrees` | counts plus confidence |
| `triage::match_todos_to_issues` | `matches`, JSON `fetch_issues.output` | `{matched}` |
| `triage::identify_untracked` **score** | `matched` | `{untracked,total,untracked_count}` plus confidence |
| `triage::match_plans_to_commits` | `frontmatter`, `recent_commits.output` | `{plans}` with match flags |
| `triage::detect_status_mismatch` **score** | `plans` | `{mismatches,total_plans}` plus confidence |
| `triage::categorize_commits` | `items[*].output` | conventional-commit category map |
| `triage::classify_true_false` **score** | finding `items` | `{true_positives,false_positives}` plus confidence |
| `triage::generate_allowlist_entries` | `false_positives` | `{allowlist_entries}` |

## SQLite and tasks

All SQLite handlers require args `db`, `sql`; optional `params` is a named
parameter object. `sqlite::exec`, `sqlite::update`, `sqlite::delete`, and
`sqlite::upsert` return `{rows_affected}`. `sqlite::insert` returns
`{last_insert_rowid}`. `sqlite::query_many` returns `{rows}`.
`sqlite::query_one` returns `{row}` and requires exactly one row.

Task handlers require args `db` (redb path):

| Handler | Additional args | Output |
| --- | --- | --- |
| `task::create` | `title`; optional `description`, `priority`, `status`, `labels` | `{id}` |
| `task::update` | `id`; optional `status`, `priority` | `{updated}` |
| `task::list` | optional `status`, `priority`, `label` | task array |
| `task::ready` | none | ready-task array |

## BAML-routed handlers

Every `llm::*` handler below is BAML-routed and always registered — there is no
feature flag. They prefer a local Ollama and need no API key when one is
reachable, falling back to hosted providers otherwise. All of them read their
fields from the step's `args` object (top-level keys are also accepted) and
ignore whatever the previous step produced, so they ignore `input_schema`.

| Handler | Additional args | Output |
| --- | --- | --- |
| `llm::analyze` | `subject`, `evidence`; optional `focus`, `client` | `{summary, findings[{title, detail, severity, evidence}], recommendation, confidence}` — emits confidence |
| `llm::confidence` | `claim`, `evidence`; optional `criteria[]`, `client` | `{score, level, reasoning, factors[]}` — score becomes the step confidence |
| `llm::extract` | top-level `function`, object `input`; optional `client` | Function-specific; functions are `ExtractEntities`, `Summarize`, `Classify`, `DescribeProject`, `AssessHealth`, `ClassifyProject`, `GenerateChangelog`, `SuggestRelated`, `ClassifyCIFailure` |
| `llm::decompose` | top-level `text` | `{tasks}` |
| `llm::plan` | `goal`; optional `constraints` | `{pipeline_name, yaml}` |

Every one of these emits a step confidence, so any of them can feed
`route_on_confidence` directly. The score is
`self_report * 0.8^(attempts - 1)`: the model's own `confidence` field, discounted
by how many attempts BAML needed to reach a parseable answer. `attempts` is in the
payload so a trace shows why. The self-report is only loosely calibrated, so prefer
`llm::confidence` when the decision needs real calibration. See
`examples/invoke_confidence.crux` and `examples/analyze_review.crux`.

## Caller-supplied output schema (`llm::invoke`)

`llm::invoke`, `llm::stream`, and `llm::invoke_with_fallback` take an optional
`schema` arg. BAML injects those fields into the completion's output type, so the
model is constrained to that exact shape and the result is validated rather than
prose. Omit `schema` for free text.

```yaml
- step: review
  handler: llm::invoke
  args:
    prompt: "Review this diff:\n\n{{ steps.diff.output.stdout }}"
    schema:
      type: object
      properties:
        verdict: { type: string, enum: [approve, comment, block] }
        blockers: { type: array, items: { type: string } }
        score: { type: number }
```

A bare `{field: {schema}}` map is accepted as shorthand for a full JSON Schema.
Values must be schemas, not samples — a bare `"high"` is ambiguous between a type
and an enum. Supported: `string`, `number`, `integer`, `boolean`, `null`, `array`,
object, string `enum`/`const`, `anyOf`/`oneOf`, and local `$defs` `$ref`.
Anything else is rejected with a message naming the offending field. See
`examples/structured_review.crux`.
