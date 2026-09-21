#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SchemaFormat {
    Json,
    Yaml,
}

pub fn cmd_schema(format: SchemaFormat) {
    let schema = crux_script::pipeline_json_schema();
    let rendered = match format {
        SchemaFormat::Json => {
            serde_json::to_string_pretty(&schema).map_err(|error| error.to_string())
        }
        SchemaFormat::Yaml => serde_yaml::to_string(&schema).map_err(|error| error.to_string()),
    };
    match rendered {
        Ok(output) => print!("{output}"),
        Err(error) => {
            eprintln!("failed to render pipeline schema: {error}");
            std::process::exit(1);
        }
    }
}
