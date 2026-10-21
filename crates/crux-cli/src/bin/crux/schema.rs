//! `crux schema` — export the `.crux` JSON Schema and editor config snippets.

use std::path::Path;

/// Default location, relative to a project root, for the exported schema.
pub const DEFAULT_SCHEMA_REL_PATH: &str = ".vscode/crux-schema.json";

/// File patterns the schema applies to: pipeline files and Cruxfiles.
const SCHEMA_FILE_PATTERNS: [&str; 2] = ["*.crux", "Cruxfile"];

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SchemaFormat {
    Json,
    Yaml,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SchemaEditor {
    /// `yaml.schemas` snippet for VS Code and any yaml-language-server client
    #[value(name = "yaml-language-server", alias = "vscode")]
    YamlLanguageServer,
}

/// The `.crux` pipeline schema as pretty-printed JSON.
pub fn pipeline_schema_json() -> Result<String, String> {
    serde_json::to_string_pretty(&crux_script::pipeline_json_schema())
        .map_err(|error| error.to_string())
}

/// Editor settings that register `schema_rel_path` as the schema for `.crux`
/// files and Cruxfiles. Returns a complete JSON object suitable for both stdout
/// and a `.vscode/settings.json` file.
///
/// Pure: `schema_rel_path` is echoed verbatim, so callers control whether it is
/// workspace-relative (`./.vscode/crux-schema.json`) or absolute.
pub fn editor_settings_snippet(schema_rel_path: &str) -> String {
    let mut out = String::from("{\n  \"yaml.schemas\": {\n");
    out.push_str(&format!("    \"{schema_rel_path}\": ["));
    out.push_str(
        &SCHEMA_FILE_PATTERNS
            .iter()
            .map(|pattern| format!("\"{pattern}\""))
            .collect::<Vec<_>>()
            .join(", "),
    );
    out.push_str("]\n  }\n}\n");
    out
}

pub fn cmd_schema(format: SchemaFormat, output: Option<&str>, editor: Option<SchemaEditor>) {
    if let Some(editor) = editor {
        print!("{}", render_editor_snippet(editor));
        std::process::exit(0);
    }

    let rendered = match format {
        SchemaFormat::Json => pipeline_schema_json(),
        SchemaFormat::Yaml => {
            serde_saphyr::to_string(&crux_script::pipeline_json_schema()).map_err(|e| e.to_string())
        }
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

/// Emits an editor snippet plus the prerequisite command that exports the
/// schema file it points at.
fn render_editor_snippet(editor: SchemaEditor) -> String {
    match editor {
        SchemaEditor::YamlLanguageServer => {
            let mut out = String::from(
                "# Write to .vscode/settings.json. Export the schema first:\n\
                 #   crux schema -o ",
            );
            out.push_str(DEFAULT_SCHEMA_REL_PATH);
            out.push('\n');
            out.push_str(&editor_settings_snippet(&format!(
                "./{DEFAULT_SCHEMA_REL_PATH}"
            )));
            out
        }
    }
}

/// Writes the schema and editor settings into a scaffolded project root.
pub fn scaffold_editor_config(root: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(root.join(".vscode"))?;
    let schema = pipeline_schema_json()
        .map_err(|error| std::io::Error::other(format!("failed to render schema: {error}")))?;
    std::fs::write(root.join(DEFAULT_SCHEMA_REL_PATH), schema)?;
    std::fs::write(
        root.join(".vscode/settings.json"),
        editor_settings_snippet(&format!("./{DEFAULT_SCHEMA_REL_PATH}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_schema_json_round_trips_as_json() {
        let rendered = pipeline_schema_json().expect("schema renders");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");
        assert_eq!(parsed["title"], "PipelineDef");
    }

    #[test]
    fn editor_settings_associate_pipelines_and_cruxfiles() {
        let snippet = editor_settings_snippet("./.vscode/crux-schema.json");
        let parsed: serde_json::Value = serde_json::from_str(&snippet).expect("valid JSON");
        let patterns = &parsed["yaml.schemas"]["./.vscode/crux-schema.json"];
        assert_eq!(patterns[0], "*.crux");
        assert_eq!(patterns[1], "Cruxfile");
    }

    #[test]
    fn editor_snippet_echoes_the_given_relative_path() {
        let snippet = editor_settings_snippet("/abs/path/schema.json");
        assert!(snippet.contains("/abs/path/schema.json"), "{snippet}");
        assert!(serde_json::from_str::<serde_json::Value>(&snippet).is_ok());
    }

    #[test]
    fn render_editor_snippet_includes_the_export_command() {
        let rendered = render_editor_snippet(SchemaEditor::YamlLanguageServer);
        assert!(rendered.contains("crux schema -o"), "{rendered}");
        assert!(rendered.contains(DEFAULT_SCHEMA_REL_PATH), "{rendered}");
    }

    #[test]
    fn scaffold_writes_schema_and_settings() {
        let root = tempfile::tempdir().expect("tempdir");
        scaffold_editor_config(root.path()).expect("scaffold succeeds");

        let schema_path = root.path().join(DEFAULT_SCHEMA_REL_PATH);
        let schema: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&schema_path).expect("read"))
                .expect("JSON");
        assert_eq!(schema["title"], "PipelineDef");

        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.path().join(".vscode/settings.json")).expect("read"),
        )
        .expect("JSON");
        assert!(settings["yaml.schemas"].is_object());
    }
}
