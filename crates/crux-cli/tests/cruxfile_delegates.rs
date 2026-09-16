use std::io::Write as _;
use std::process::{Command, Output};

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

fn run_cruxfile(strict: bool) -> Output {
    let mut file = tempfile::NamedTempFile::new().expect("temporary Cruxfile must be created");
    file.write_all(CRUXFILE.as_bytes())
        .expect("temporary Cruxfile must be written");
    let mut command = Command::new(env!("CARGO_BIN_EXE_crux"));
    command.arg("run").arg(file.path());
    if strict {
        command.arg("--strict");
    }
    command.output().expect("crux run must execute")
}

#[test]
fn cruxfile_registers_stub_for_nested_delegate() {
    let output = run_cruxfile(false);
    assert!(
        output.status.success(),
        "crux run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("agent 'missing-agent', using stub"));
}

#[test]
fn strict_cruxfile_rejects_nested_unregistered_delegate() {
    let output = run_cruxfile(true);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--strict mode: unregistered agents: missing-agent")
    );
}
