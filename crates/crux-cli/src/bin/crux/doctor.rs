pub fn cmd_doctor(plugins: Option<&str>) {
    let runtime = tokio::runtime::Runtime::new().expect("Tokio runtime");
    let registry = runtime.block_on(super::registry::build_base_registry(plugins));
    let handler_count = registry.handler_metadata().len();
    println!("ok  handler registry: {handler_count} handler(s)");

    let model_vars = ["OPENAI_API_KEY", "ANTHROPIC_API_KEY"];
    let configured = model_vars
        .iter()
        .filter(|name| std::env::var(name).is_ok_and(|value| !value.is_empty()))
        .count();
    println!(
        "{}  model configuration: {configured} provider key(s) present (values hidden)",
        if configured > 0 { "ok" } else { "--" }
    );

    let docker = std::process::Command::new("docker")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    println!(
        "{}  Docker: {}",
        if docker { "ok" } else { "--" },
        if docker { "available" } else { "not available" }
    );

    let plugin_path = super::registry::resolve_plugins_path(plugins);
    println!(
        "{}  plugins: {}",
        if std::path::Path::new(&plugin_path).exists() {
            "ok"
        } else {
            "--"
        },
        plugin_path
    );
    println!(
        "{}  BAML capability: {}",
        if cfg!(feature = "baml") { "ok" } else { "--" },
        if cfg!(feature = "baml") {
            "enabled"
        } else {
            "disabled"
        }
    );
}
