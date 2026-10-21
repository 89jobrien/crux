//! Coverage tests for the aggregate built-in handler registry.

use crux_script::HandlerRegistry;

#[test]
fn register_all_installs_expected_handlers() {
    let mut reg = HandlerRegistry::new();
    crux_agentic::register_all(&mut reg);

    let expected = [
        "ctrl::log",
        "ctrl::noop",
        "ctrl::assert",
        "shell::exec",
        "shell::capture",
        "fs::read",
        "fs::write",
        "fs::glob",
        "fs::exists",
        "git::staged_files",
        "git::diff",
        "git::log",
        "git::status",
        "json::pick",
        "json::merge",
        "json::jq",
        "http::request",
        "llm::invoke",
        "analysis::latency_profile",
        "analysis::token_spend",
        "analysis::failure_clusters",
        "analysis::replay_cache_hits",
        "analysis::tighten_budget",
        "analysis::compress_stages",
        "analysis::tune_retry",
        "analysis::patch_schema_check",
        "analysis::replay_dry_run",
        "ci::compile_errors",
        "ci::clippy_violations",
        "ci::nextest_failures",
        "ci::deny_violations",
        "ci::deduplicate_spans",
        "ci::classify_severity",
        "ci::attach_owners",
        "ci::score_fixability",
        "review::arch_boundary_check",
        "review::normalize_findings",
        "review::apply_severity",
        "review::compute_score",
        "review::approve",
        "triage::parse_repo_tags",
        "triage::score_urgency",
        "triage::deduplicate_intent",
        "triage::group_by_repo",
    ];

    for name in &expected {
        assert!(reg.get_handler(name).is_some(), "missing handler: {name}");
    }
}

/// BAML is a mandatory dependency, so `register_all` always installs the
/// BAML-routed LLM handlers. This list is the contract: every `llm::*` handler
/// that makes a model call must be present, and each must be BAML-backed.
#[test]
fn register_all_installs_baml_handlers() {
    let mut reg = HandlerRegistry::new();
    crux_agentic::register_all(&mut reg);

    let baml_expected = [
        "llm::invoke",
        "llm::invoke_with_fallback",
        "llm::stream",
        "llm::extract",
        "llm::analyze",
        "llm::confidence",
        "llm::decompose",
        "llm::plan",
    ];

    for name in &baml_expected {
        assert!(
            reg.get_handler(name).is_some(),
            "missing BAML handler: {name}"
        );
    }
}

/// The BAML handlers must win over any non-BAML implementation, which is what
/// makes "every LLM call goes through BAML" a checked property rather than a
/// convention. `HandlerMetadata` carries the output schema, so a BAML-provided
/// contract is the observable signal.
#[test]
fn baml_llm_handlers_declare_output_contracts() {
    let mut reg = HandlerRegistry::new();
    crux_agentic::register_all(&mut reg);

    for name in [
        "llm::invoke",
        "llm::stream",
        "llm::analyze",
        "llm::confidence",
    ] {
        let metadata = reg
            .get_metadata(name)
            .unwrap_or_else(|| panic!("{name} must be registered"));
        assert!(
            metadata.output_schema.is_some(),
            "{name} must declare an output schema, proving it is the BAML handler"
        );
    }
}
