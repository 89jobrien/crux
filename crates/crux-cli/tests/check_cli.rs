use std::io::Write as _;
use std::process::{Command, Output};

use tempfile::NamedTempFile;

fn pipeline(contents: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temporary pipeline must be created");
    file.write_all(contents.as_bytes())
        .expect("temporary pipeline must be written");
    file
}

fn check(file: &NamedTempFile, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_crux"));
    command.arg("check").args(args).arg(file.path());
    command.output().expect("crux check must execute")
}

#[test]
fn check_is_a_first_class_subcommand() {
    let file = pipeline("pipeline: valid\nsteps: []\n");
    let output = check(&file, &[]);

    assert!(
        output.status.success(),
        "crux check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("ok"));
}

#[test]
fn check_strict_rejects_missing_input_schema() {
    let file = pipeline("pipeline: missing-schema\nsteps: []\n");
    let output = check(&file, &["--strict"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing_input_schema"), "{stderr}");
    assert!(stderr.contains("input_schema"), "{stderr}");
}

#[test]
fn check_renders_coded_diagnostics() {
    let file = pipeline(
        "pipeline: unknown-handler\ninput_schema:\n  type: dynamic\nsteps:\n  - step: call\n    handler: vendor::missing\n",
    );
    let output = check(&file, &["--strict"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error[unknown_handler]"), "{stderr}");
    assert!(stderr.contains("steps[0]"), "{stderr}");
}

#[test]
fn check_accepts_plugin_manifest_argument() {
    let file = pipeline("pipeline: valid\nsteps: []\n");
    let plugins = NamedTempFile::new().expect("temporary plugin manifest must be created");
    let output = check(
        &file,
        &[
            "--plugins",
            plugins
                .path()
                .to_str()
                .expect("temporary path must be UTF-8"),
        ],
    );

    assert!(
        output.status.success(),
        "crux check --plugins failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn permissive_check_warns_when_pipeline_is_not_executable() {
    let file =
        pipeline("pipeline: external\nsteps:\n  - step: call\n    handler: vendor::missing\n");
    let output = check(&file, &[]);

    assert!(
        output.status.success(),
        "permissive check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("warning[unknown_handler]"), "{stderr}");
    assert!(stderr.contains("not executable"), "{stderr}");
}

#[test]
fn run_check_remains_an_alias() {
    let file = pipeline("pipeline: missing-schema\nsteps: []\n");
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .arg("run")
        .arg(file.path())
        .arg("--check")
        .arg("--strict")
        .output()
        .expect("crux run --check must execute");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing_input_schema"));
}

#[test]
fn check_json_emits_machine_readable_diagnostics() {
    let file = pipeline(
        "pipeline: invalid\ninput_schema:\n  type: dynamic\nsteps:\n  - step: missing\n    handler: plugin::missing\n",
    );
    let output = check(&file, &["--strict", "--json"]);
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report[0]["code"], "unknown_handler");
    assert_eq!(report[0]["severity"], "error");
}
