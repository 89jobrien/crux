use std::process::{Command, Output};

use tempfile::TempDir;

fn pipeline(contents: &str) -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("pipeline.crux");
    std::fs::write(&path, contents).expect("write pipeline");
    (dir, path)
}

fn run(path: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_crux"))
        .arg("run")
        .arg(path)
        .args(args)
        .output()
        .expect("run crux")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

const EMPTY_PIPELINE: &str = "pipeline: contract\nsteps: []\n";

#[test]
fn success_modes_preserve_stream_and_exit_contracts() {
    let (_dir, path) = pipeline(EMPTY_PIPELINE);

    let default = run(&path, &[]);
    assert!(default.status.success());
    assert!(text(&default.stdout).contains("contract  PASS"));
    assert!(text(&default.stdout).contains("0/0 checks passed"));
    assert_ne!(text(&default.stdout), "null\n");
    assert_eq!(text(&default.stderr), "");

    let summary = run(&path, &["--summary"]);
    assert!(summary.status.success());
    assert!(text(&summary.stdout).contains("contract  PASS"));
    assert!(text(&summary.stdout).contains("0/0 checks passed"));
    assert_eq!(text(&summary.stderr), "");

    let verbose = run(&path, &["--verbose"]);
    assert!(verbose.status.success());
    assert!(text(&verbose.stdout).contains("Pipeline: contract"));
    assert!(text(&verbose.stdout).contains("Trace:"));
    assert_eq!(text(&verbose.stderr), "");

    let json = run(&path, &["--json"]);
    assert!(json.status.success());
    assert_eq!(text(&json.stdout), "null\n");
    assert_eq!(text(&json.stderr), "");

    let quiet = run(&path, &["--quiet"]);
    assert!(quiet.status.success());
    assert_eq!(text(&quiet.stdout), "");
    assert_eq!(text(&quiet.stderr), "");
}

#[test]
fn default_and_verbose_humanize_successful_shell_output() {
    let (_dir, path) = pipeline(
        r#"pipeline: shell-output
steps:
  - step: build
    handler: shell::capture
    args:
      cmd: "printf 'Compiling crux\nFinished test profile\n'; printf 'warning: build noise\n' >&2"
"#,
    );

    let default = run(&path, &[]);
    let default_stdout = text(&default.stdout);
    assert!(default.status.success());
    assert!(
        default_stdout.contains("shell-output  PASS"),
        "{default_stdout}"
    );
    assert!(
        default_stdout.contains("Output:\nCompiling crux\nFinished test profile\n"),
        "{default_stdout}"
    );
    assert!(!default_stdout.contains("exit_code"), "{default_stdout}");
    assert!(
        !default_stdout.contains("warning: build noise"),
        "{default_stdout}"
    );
    assert!(!default_stdout.contains(r"\n"), "{default_stdout}");

    let verbose = run(&path, &["--verbose"]);
    let verbose_stdout = text(&verbose.stdout);
    assert!(verbose.status.success());
    assert!(
        verbose_stdout.contains("Pipeline: shell-output"),
        "{verbose_stdout}"
    );
    assert!(verbose_stdout.contains("Trace:"), "{verbose_stdout}");
    assert!(
        verbose_stdout.contains("Output:\nCompiling crux\nFinished test profile\n"),
        "{verbose_stdout}"
    );
    assert!(!verbose_stdout.contains("exit_code"), "{verbose_stdout}");
    assert!(
        !verbose_stdout.contains("warning: build noise"),
        "{verbose_stdout}"
    );
    assert!(!verbose_stdout.contains(r"\n"), "{verbose_stdout}");
    assert_eq!(text(&verbose.stderr), "");
}

#[test]
fn budget_failure_has_one_diagnostic_and_exit_one() {
    let (_dir, path) = pipeline(
        "pipeline: budget-failure\nbudget: { steps: 0 }\nsteps:\n  - step: blocked\n    handler: ctrl::noop\n",
    );

    let output = run(&path, &[]);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(text(&output.stderr), "");
    assert!(stdout.contains("budget-failure  FAIL"), "{stdout}");
    assert_eq!(
        stdout.matches("step budget exceeded").count(),
        1,
        "{stdout}"
    );
}

#[test]
fn summary_failure_prints_exactly_one_diagnostic() {
    let (_dir, path) = pipeline(
        "pipeline: summary-failure\nbudget: { steps: 0 }\nsteps:\n  - step: blocked\n    handler: ctrl::noop\n",
    );

    let output = run(&path, &["--summary"]);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(text(&output.stderr), "");
    assert_eq!(
        stdout.matches("step budget exceeded").count(),
        1,
        "{stdout}"
    );
}

#[test]
fn invalid_output_flags_exit_two_with_one_parser_error() {
    let (_dir, path) = pipeline(EMPTY_PIPELINE);
    let output = run(&path, &["--summary", "--verbose"]);
    let stderr = text(&output.stderr);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(text(&output.stdout), "");
    assert!(stderr.contains("cannot be used with"), "{stderr}");
    assert_eq!(stderr.matches("error:").count(), 1, "{stderr}");
}

#[test]
fn explicit_json_rejects_cruxfile_targets_with_exit_two() {
    let (_dir, path) =
        pipeline("project: contract\ndefault: all\ntargets:\n  all:\n    steps: []\n");
    let output = run(&path, &["--json"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(text(&output.stdout), "");
    assert_eq!(
        text(&output.stderr),
        "error: --json is not supported for Cruxfile targets\n"
    );
}
