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

#[cfg(feature = "baml")]
#[test]
fn register_all_installs_baml_handlers() {
    let mut reg = HandlerRegistry::new();
    crux_agentic::register_all(&mut reg);

    let baml_expected = ["llm::extract", "llm::decompose", "llm::plan"];

    for name in &baml_expected {
        assert!(
            reg.get_handler(name).is_some(),
            "missing BAML handler: {name}"
        );
    }
}
