use std::process::Command;

#[test]
fn replay_debugger_steps_through_serialized_trace() {
    let trace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/fixtures/showcase_trace.json");
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args(["replay-debug", trace.to_str().unwrap(), "--step", "0"])
        .output()
        .expect("crux replay-debug must execute");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(report.contains("gather_data::read_metrics"), "{report}");
    assert!(report.contains("input hash"), "{report}");
}
