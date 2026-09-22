use std::process::Command;

#[test]
fn trace_command_filters_and_renders_timeline() {
    let trace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/fixtures/showcase_trace.json");
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args(["trace", trace.to_str().unwrap(), "--status", "ok"])
        .output()
        .expect("crux trace must execute");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let timeline = String::from_utf8(output.stdout).unwrap();
    assert!(timeline.contains("Timeline"), "{timeline}");
    assert!(timeline.contains("gather_data::read_metrics"), "{timeline}");
    assert!(timeline.contains("Live"), "{timeline}");
}

#[test]
fn trace_command_exports_mermaid_graph() {
    let trace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/fixtures/showcase_trace.json");
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args(["trace", trace.to_str().unwrap(), "--mermaid"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .starts_with("graph TD")
    );
}
