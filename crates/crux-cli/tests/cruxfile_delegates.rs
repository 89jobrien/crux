use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output};

use crux_runtime::prelude::Crux;
use serde_json::Value;
use tempfile::TempDir;

const CRUXFILE: &str = r#"project: delegated-project
default: build
targets:
  build:
    steps:
      - repeat: delegated-loop
        count: 1
        steps:
          - delegate: missing-agent
"#;

const MULTI_TARGET_CRUXFILE: &str = r#"project: trace-project
default: test
targets:
  build:
    steps: []
  test:
    depends: [build]
    steps: []
"#;

fn run_cruxfile(contents: &str, strict: bool) -> (TempDir, Output) {
    let home = tempfile::tempdir().expect("temporary home must be created");
    let path = home.path().join("Cruxfile");
    let mut file = std::fs::File::create(&path).expect("temporary Cruxfile must be created");
    file.write_all(contents.as_bytes())
        .expect("temporary Cruxfile must be written");
    drop(file);

    let mut command = Command::new(env!("CARGO_BIN_EXE_crux"));
    command.arg("run").arg(&path).env("HOME", home.path());
    if strict {
        command.arg("--strict");
    }
    let output = command.output().expect("crux run must execute");
    (home, output)
}

fn trace_files(home: &TempDir) -> Vec<PathBuf> {
    let trace_dir = home.path().join(".crux").join("traces");
    if !trace_dir.exists() {
        return Vec::new();
    }

    let mut files: Vec<_> = std::fs::read_dir(trace_dir)
        .expect("trace directory must be readable")
        .map(|entry| entry.expect("trace entry must be readable").path())
        .collect();
    files.sort();
    files
}

fn read_trace(path: &std::path::Path) -> Crux<Value> {
    let contents = std::fs::read_to_string(path).expect("trace file must be readable");
    serde_json::from_str(&contents).expect("trace file must be replayable JSON")
}

#[test]
fn cruxfile_persists_one_trace_per_executed_target() {
    let (home, output) = run_cruxfile(MULTI_TARGET_CRUXFILE, false);
    assert!(
        output.status.success(),
        "crux run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let traces = trace_files(&home);
    assert_eq!(traces.len(), 2);
    let names: Vec<_> = traces
        .iter()
        .map(|path| {
            path.file_name()
                .expect("trace path must have a file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(
        names
            .iter()
            .any(|name| name.contains("trace-project-build"))
    );
    assert!(names.iter().any(|name| name.contains("trace-project-test")));
    assert!(traces.iter().all(|path| read_trace(path).value().is_ok()));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr)
            .matches("[crux] trace saved to ")
            .count(),
        2
    );
}

#[test]
fn cruxfile_registers_stub_for_nested_delegate() {
    let (_home, output) = run_cruxfile(CRUXFILE, false);
    assert!(
        output.status.success(),
        "crux run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("agent 'missing-agent', using stub"));
}

#[test]
fn strict_cruxfile_rejects_nested_unregistered_delegate() {
    let (_home, output) = run_cruxfile(CRUXFILE, true);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--strict mode: unregistered agents: missing-agent")
    );
}
