#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SchemaFormat {
    Json,
    Yaml,
}

pub fn cmd_schema(format: SchemaFormat, output: Option<&str>) {
    let schema = crux_script::pipeline_json_schema();
    let rendered = match format {
        SchemaFormat::Json => {
            serde_json::to_string_pretty(&schema).map_err(|error| error.to_string())
        }
        SchemaFormat::Yaml => serde_yaml::to_string(&schema).map_err(|error| error.to_string()),
    };
    match (rendered, output) {
        (Ok(rendered), Some(path)) => {
            if let Err(error) = std::fs::write(path, rendered) {
                eprintln!("failed to write pipeline schema to {path}: {error}");
                std::process::exit(1);
            }
        }
        (Ok(rendered), None) => print!("{rendered}"),
        (Err(error), _) => {
            eprintln!("failed to render pipeline schema: {error}");
            std::process::exit(1);
        }
    }
}
