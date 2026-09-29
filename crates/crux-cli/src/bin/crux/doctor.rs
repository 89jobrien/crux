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
    // BAML is a mandatory dependency, so every LLM call is BAML-routed. Report
    // which backend will actually serve it.
    let ollama = std::net::TcpStream::connect("127.0.0.1:11434").is_ok();
    let openai = std::env::var("OPENAI_API_KEY").is_ok_and(|v| !v.is_empty());
    let anthropic = std::env::var("ANTHROPIC_API_KEY").is_ok_and(|v| !v.is_empty());
    let (backend, detail) = match (ollama, openai, anthropic) {
        (true, _, _) => ("ok", "ollama (local)"),
        (false, true, true) => ("ok", "openai + anthropic"),
        (false, true, false) => ("ok", "openai"),
        (false, false, true) => ("ok", "anthropic"),
        (false, false, false) => ("--", "no backend reachable"),
    };
    println!("{backend}  LLM backend: {detail}");
}
