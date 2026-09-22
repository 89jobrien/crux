use std::process::Command;

#[test]
fn doctor_reports_environment_capabilities_without_secrets() {
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .arg("doctor")
        .env("OPENAI_API_KEY", "super-secret-value")
        .output()
        .expect("crux doctor must execute");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(report.contains("handler registry"), "{report}");
    assert!(report.contains("Docker"), "{report}");
    assert!(report.contains("model configuration"), "{report}");
    assert!(!report.contains("super-secret-value"), "{report}");
}
