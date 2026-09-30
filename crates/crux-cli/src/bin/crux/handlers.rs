#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum HandlerFormat {
    Markdown,
    Json,
}

pub fn cmd_handlers(format: HandlerFormat, plugins: Option<&str>) {
    let runtime = tokio::runtime::Runtime::new().expect("Tokio runtime");
    let registry = runtime.block_on(super::registry::build_base_registry(plugins));
    let metadata = registry.handler_metadata();
    match format {
        HandlerFormat::Json => println!("{}", serde_json::to_string_pretty(&metadata).unwrap()),
        HandlerFormat::Markdown => {
            println!("# Handler catalog\n");
            for handler in metadata {
                println!("## `{}`\n", handler.name);
                if !handler.description.is_empty() {
                    println!("{}\n", handler.description);
                }
                println!("- **Risk:** `{:?}`", handler.risk);
                println!("- **Deterministic:** `{}`", handler.deterministic);
                println!("- **Capabilities:** `{:?}`", handler.capabilities);
                println!("- **Side effects:** `{:?}`\n", handler.side_effects);
                println!("### Arguments\n");
                if handler.args.args.is_empty() {
                    println!("No declared arguments.\n");
                } else {
                    println!("| Name | Required | Schema | Description |");
                    println!("| --- | --- | --- | --- |");
                    for argument in &handler.args.args {
                        println!(
                            "| `{}` | {} | `{:?}` | {} |",
                            argument.name,
                            argument.required,
                            argument.schema,
                            argument.description.as_deref().unwrap_or("")
                        );
                    }
                    println!();
                }
                println!("### Output\n");
                println!("`{:?}`\n", handler.output_schema);
            }
        }
    }
}
