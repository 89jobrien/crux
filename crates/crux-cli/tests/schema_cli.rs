use std::process::Command;

#[test]
fn schema_defaults_to_json_and_includes_control_flow_nodes() {
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .arg("schema")
        .output()
        .expect("crux schema must execute");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let schema: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let rendered = schema.to_string();
    for field in [
        "expect",
        "allow_failure",
        "timeout_ms",
        "retry",
        "on_error",
        "vars",
        "poll",
        "for_each",
        "while",
        "repeat",
    ] {
        assert!(rendered.contains(field), "schema omitted {field}");
    }
}

#[test]
fn schema_can_render_yaml() {
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args(["schema", "--format", "yaml"])
        .output()
        .expect("crux schema --format yaml must execute");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered = String::from_utf8(output.stdout).unwrap();
    assert!(rendered.contains("$schema:"), "{rendered}");
    assert!(rendered.contains("PipelineDef"), "{rendered}");
}

#[test]
fn schema_can_write_editor_schema_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("crux.schema.json");
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args(["schema", "--output"])
        .arg(&path)
        .output()
        .expect("crux schema --output must execute");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let schema: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(schema["title"], "PipelineDef");
}

#[test]
fn schema_editor_snippet_registers_pipelines_and_cruxfiles() {
    for editor in ["yaml-language-server", "vscode"] {
        let output = Command::new(env!("CARGO_BIN_EXE_crux"))
            .args(["schema", "--editor", editor])
            .output()
            .expect("crux schema --editor must execute");

        assert!(
            output.status.success(),
            "{editor}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rendered = String::from_utf8(output.stdout).unwrap();
        assert!(rendered.contains("crux schema -o"), "{editor}: {rendered}");

        // The snippet carries leading comments, so parse only the JSON body.
        let json = rendered
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        let settings: serde_json::Value = serde_json::from_str(&json).unwrap_or_else(|error| {
            panic!("{editor}: snippet is not valid JSON ({error}): {json}")
        });
        let patterns = &settings["yaml.schemas"]["./.vscode/crux-schema.json"];
        assert_eq!(patterns[0], "*.crux", "{editor}");
        assert_eq!(patterns[1], "Cruxfile", "{editor}");
    }
}

#[test]
fn init_scaffolds_editor_schema_wiring() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .arg("init")
        .arg(temp.path())
        .output()
        .expect("crux init must execute");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let schema: serde_json::Value = serde_json::from_slice(
        &std::fs::read(temp.path().join(".vscode/crux-schema.json")).expect("schema file"),
    )
    .expect("scaffolded schema must be valid JSON");
    assert_eq!(schema["title"], "PipelineDef");

    let settings: serde_json::Value = serde_json::from_slice(
        &std::fs::read(temp.path().join(".vscode/settings.json")).expect("settings file"),
    )
    .expect("scaffolded settings must be valid JSON");
    assert!(
        settings["yaml.schemas"]["./.vscode/crux-schema.json"].is_array(),
        "{settings}"
    );
}
