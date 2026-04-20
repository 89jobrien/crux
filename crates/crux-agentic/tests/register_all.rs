use cruxai_script::HandlerRegistry;

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
