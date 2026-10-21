use std::fs;
use std::process::Command;

fn write_pipeline(directory: &std::path::Path) -> std::path::PathBuf {
    let pipeline = directory.join("bounded.crux");
    fs::write(
        &pipeline,
        r#"pipeline: bounded
steps:
  - step: first
    handler: ctrl::assert
    args:
      condition: true
      message: first failed
  - step: second
    handler: ctrl::assert
    args:
      condition: true
      message: second failed
  - step: third
    handler: ctrl::assert
    args:
      condition: true
      message: third failed
"#,
    )
    .unwrap();
    pipeline
}

#[test]
fn through_persists_prefix_and_replays_only_prior_steps() {
    let directory = tempfile::tempdir().unwrap();
    let pipeline = write_pipeline(directory.path());
    let first_trace = directory.path().join("first.json");
    let second_trace = directory.path().join("second.json");

    let first = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args([
            "run",
            pipeline.to_str().unwrap(),
            "--through",
            "first",
            "--quiet",
        ])
        .args(["--save-trace", first_trace.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );

    let first_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&first_trace).unwrap()).unwrap();
    assert_eq!(first_json["steps"].as_array().unwrap().len(), 1);
    assert_eq!(first_json["steps"][0]["name"], "first");
    assert_eq!(first_json["steps"][0]["origin"], "live");

    let second = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args([
            "run",
            pipeline.to_str().unwrap(),
            "--through",
            "second",
            "--replay",
            first_trace.to_str().unwrap(),
            "--quiet",
        ])
        .args(["--save-trace", second_trace.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );

    let second_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&second_trace).unwrap()).unwrap();
    assert_eq!(second_json["steps"].as_array().unwrap().len(), 2);
    assert_eq!(second_json["steps"][0]["origin"], "replayed");
    assert_eq!(second_json["steps"][0]["duration_ms"], 0);
    assert_eq!(second_json["steps"][1]["name"], "second");
    assert_eq!(second_json["steps"][1]["origin"], "live");
}

#[test]
fn through_rejects_unknown_boundary_and_documents_top_level_scope() {
    let directory = tempfile::tempdir().unwrap();
    let pipeline = write_pipeline(directory.path());
    let trace = directory.path().join("unknown.json");

    let unknown = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args([
            "run",
            pipeline.to_str().unwrap(),
            "--through",
            "missing",
            "--quiet",
        ])
        .args(["--save-trace", trace.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&unknown.stderr)
            .contains("unknown top-level step boundary 'missing'")
    );

    let help = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args(["run", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("top-level step"));
}

#[test]
fn through_rejects_dry_run_and_cruxfile_targets() {
    let directory = tempfile::tempdir().unwrap();
    let pipeline = write_pipeline(directory.path());
    let dry_run = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args([
            "run",
            pipeline.to_str().unwrap(),
            "--through",
            "first",
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert_eq!(dry_run.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&dry_run.stderr).contains("cannot be combined"));

    let cruxfile = directory.path().join("Cruxfile");
    fs::write(
        &cruxfile,
        "project: bounded\ndefault: build\ntargets:\n  build:\n    steps: []\n",
    )
    .unwrap();
    let target = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args(["run", cruxfile.to_str().unwrap(), "--through", "build"])
        .output()
        .unwrap();
    assert_eq!(target.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&target.stderr).contains("Cruxfile targets"));
}
