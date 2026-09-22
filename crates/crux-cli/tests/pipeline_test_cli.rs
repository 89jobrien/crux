use std::{io::Write as _, process::Command};

#[test]
fn test_command_runs_pipeline_fixture_with_mocked_handlers() {
    let mut fixture = tempfile::NamedTempFile::new().unwrap();
    write!(
        fixture,
        "{}",
        serde_json::json!({
            "pipeline": "pipeline: fixture\nsteps:\n  - step: greet\n    handler: demo::greet\n",
            "input": {"name": "Joe"},
            "handlers": {"demo::greet": {"message": "hello"}},
            "expected": {"message": "hello"}
        })
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .arg("test")
        .arg(fixture.path())
        .output()
        .expect("crux test must execute");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("passed"));
}
