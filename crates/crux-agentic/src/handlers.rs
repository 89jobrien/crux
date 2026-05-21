//! Compile-time constants for all built-in handler names.
//!
//! Use these instead of raw string literals to catch typos at compile time.

// shell
pub const SHELL_EXEC: &str = "shell::exec";
pub const SHELL_CAPTURE: &str = "shell::capture";

// fs
pub const FS_READ: &str = "fs::read";
pub const FS_WRITE: &str = "fs::write";
pub const FS_GLOB: &str = "fs::glob";
pub const FS_EXISTS: &str = "fs::exists";

// git
pub const GIT_STAGED_FILES: &str = "git::staged_files";
pub const GIT_DIFF: &str = "git::diff";
pub const GIT_LOG: &str = "git::log";
pub const GIT_STATUS: &str = "git::status";

// json
pub const JSON_PICK: &str = "json::pick";
pub const JSON_MERGE: &str = "json::merge";
pub const JSON_JQ: &str = "json::jq";

// ctrl
pub const CTRL_NOOP: &str = "ctrl::noop";
pub const CTRL_LOG: &str = "ctrl::log";
pub const CTRL_ASSERT: &str = "ctrl::assert";

// llm
pub const LLM_INVOKE: &str = "llm::invoke";
pub const LLM_EXTRACT: &str = "llm::extract";
pub const LLM_DECOMPOSE: &str = "llm::decompose";
pub const LLM_PLAN: &str = "llm::plan";
pub const LLM_STREAM: &str = "llm::stream";

// container
pub const CONTAINER_RUN: &str = "container::run";
pub const CONTAINER_WAIT: &str = "container::wait";

// harness
pub const HARNESS_EVOLVE: &str = "harness::evolve";
pub const HARNESS_CANARY: &str = "harness::canary";

// rx
pub const RX_RUN: &str = "rx::run";
pub const RX_LIST: &str = "rx::list";
pub const RX_INSTALL: &str = "rx::install";

// analysis
pub const ANALYSIS_LATENCY_PROFILE: &str = "analysis::latency_profile";
pub const ANALYSIS_TOKEN_SPEND: &str = "analysis::token_spend";
pub const ANALYSIS_FAILURE_CLUSTERS: &str = "analysis::failure_clusters";
pub const ANALYSIS_REPLAY_CACHE_HITS: &str = "analysis::replay_cache_hits";
pub const ANALYSIS_TIGHTEN_BUDGET: &str = "analysis::tighten_budget";
pub const ANALYSIS_COMPRESS_STAGES: &str = "analysis::compress_stages";
pub const ANALYSIS_TUNE_RETRY: &str = "analysis::tune_retry";
pub const ANALYSIS_PATCH_SCHEMA_CHECK: &str = "analysis::patch_schema_check";
pub const ANALYSIS_REPLAY_DRY_RUN: &str = "analysis::replay_dry_run";

// ci
pub const CI_COMPILE_ERRORS: &str = "ci::compile_errors";
pub const CI_CLIPPY_VIOLATIONS: &str = "ci::clippy_violations";
pub const CI_NEXTEST_FAILURES: &str = "ci::nextest_failures";
pub const CI_DENY_VIOLATIONS: &str = "ci::deny_violations";
pub const CI_DEDUPLICATE_SPANS: &str = "ci::deduplicate_spans";
pub const CI_CLASSIFY_SEVERITY: &str = "ci::classify_severity";
pub const CI_ATTACH_OWNERS: &str = "ci::attach_owners";
pub const CI_SCORE_FIXABILITY: &str = "ci::score_fixability";

// review
pub const REVIEW_ARCH_BOUNDARY_CHECK: &str = "review::arch_boundary_check";
pub const REVIEW_NORMALIZE_FINDINGS: &str = "review::normalize_findings";
pub const REVIEW_APPLY_SEVERITY: &str = "review::apply_severity";
pub const REVIEW_COMPUTE_SCORE: &str = "review::compute_score";
pub const REVIEW_APPROVE: &str = "review::approve";
pub const REVIEW_DETECT_ANTIPATTERNS: &str = "review::detect_antipatterns";
pub const REVIEW_GROUP_BY_FILE: &str = "review::group_by_file";
pub const REVIEW_COMPOSE_DAILY_NOTE: &str = "review::compose_daily_note";

// triage
pub const TRIAGE_PARSE_REPO_TAGS: &str = "triage::parse_repo_tags";
pub const TRIAGE_SCORE_URGENCY: &str = "triage::score_urgency";
pub const TRIAGE_DEDUPLICATE_INTENT: &str = "triage::deduplicate_intent";
pub const TRIAGE_GROUP_BY_REPO: &str = "triage::group_by_repo";
pub const TRIAGE_MERGE_RESULTS: &str = "triage::merge_results";
pub const TRIAGE_PARSE_ENV_PROBE: &str = "triage::parse_env_probe";
pub const TRIAGE_CLASSIFY_SEVERITY: &str = "triage::classify_severity";
pub const TRIAGE_SUGGEST_REMEDIATION: &str = "triage::suggest_remediation";
pub const TRIAGE_CORRELATE_FAILURES: &str = "triage::correlate_failures";
pub const TRIAGE_MEASURE_OVERHEAD: &str = "triage::measure_overhead";
pub const TRIAGE_DETECT_ORPHANED_WORKTREES: &str = "triage::detect_orphaned_worktrees";
pub const TRIAGE_BUILD_CLEANUP_PLAN: &str = "triage::build_cleanup_plan";
pub const TRIAGE_MATCH_TODOS_TO_ISSUES: &str = "triage::match_todos_to_issues";
pub const TRIAGE_IDENTIFY_UNTRACKED: &str = "triage::identify_untracked";
pub const TRIAGE_MATCH_PLANS_TO_COMMITS: &str = "triage::match_plans_to_commits";
pub const TRIAGE_DETECT_STATUS_MISMATCH: &str = "triage::detect_status_mismatch";
pub const TRIAGE_CATEGORIZE_COMMITS: &str = "triage::categorize_commits";
pub const TRIAGE_CLASSIFY_TRUE_FALSE: &str = "triage::classify_true_false";
pub const TRIAGE_GENERATE_ALLOWLIST_ENTRIES: &str = "triage::generate_allowlist_entries";

// text
pub const TEXT_PARSE_VIMGREP: &str = "text::parse_vimgrep";
pub const TEXT_PARSE_JSONL: &str = "text::parse_jsonl";
pub const TEXT_PARSE_FRONTMATTER: &str = "text::parse_frontmatter";
pub const TEXT_PARSE_DIFF: &str = "text::parse_diff";
pub const TEXT_PARSE_BRANCH_LIST: &str = "text::parse_branch_list";

// json (extended)
pub const JSON_GROUP_BY: &str = "json::group_by";
pub const JSON_FILTER_NONEMPTY: &str = "json::filter_nonempty";

// sqlite
pub const SQLITE_EXEC: &str = "sqlite::exec";
pub const SQLITE_QUERY_ONE: &str = "sqlite::query_one";
pub const SQLITE_QUERY_MANY: &str = "sqlite::query_many";
pub const SQLITE_INSERT: &str = "sqlite::insert";
pub const SQLITE_UPDATE: &str = "sqlite::update";
pub const SQLITE_DELETE: &str = "sqlite::delete";
pub const SQLITE_UPSERT: &str = "sqlite::upsert";
