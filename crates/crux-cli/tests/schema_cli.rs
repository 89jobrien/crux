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
