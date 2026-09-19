use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crux_runtime::prelude::Crux;
use serde_json::Value;
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
        .env(
            "HOME",
            path.parent().expect("pipeline path must have a parent"),
        )
        .output()
        .expect("run crux")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn assert_trace_saved(stderr: &[u8]) {
    let stderr = text(stderr);
    assert_eq!(
        stderr.matches("[crux] trace saved to ").count(),
        1,
        "{stderr}"
    );
}

const EMPTY_PIPELINE: &str = "pipeline: contract\nsteps: []\n";

fn trace_files(pipeline_path: &Path) -> Vec<PathBuf> {
    let trace_dir = pipeline_path
        .parent()
        .expect("pipeline path must have a parent")
        .join(".crux")
        .join("traces");
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

fn read_trace(path: &Path) -> Crux<Value> {
    let contents = std::fs::read_to_string(path).expect("trace file must be readable");
    serde_json::from_str(&contents).expect("trace file must be replayable JSON")
}

#[test]
fn regular_runs_persist_success_failure_replay_and_override_traces() {
    let (_dir, path) = pipeline(EMPTY_PIPELINE);
    let success = run(&path, &[]);
    assert!(success.status.success());
    assert!(text(&success.stderr).contains("[crux] trace saved to "));
    let traces = trace_files(&path);
    assert_eq!(traces.len(), 1);
    assert!(read_trace(&traces[0]).value().is_ok());

    let replay_path = traces[0].to_str().expect("trace path must be UTF-8");
    let replay = run(&path, &["--replay", replay_path]);
    assert!(replay.status.success());
    assert_eq!(trace_files(&path).len(), 2);

    let (_failure_dir, failure_path) = pipeline(
        "pipeline: budget-failure\nbudget: { steps: 0 }\nsteps:\n  - step: blocked\n    handler: ctrl::noop\n",
    );
    let failure = run(&failure_path, &[]);
    assert_eq!(failure.status.code(), Some(1));
    let failed_traces = trace_files(&failure_path);
    assert_eq!(failed_traces.len(), 1);
    assert!(read_trace(&failed_traces[0]).value().is_err());

    let (_override_dir, override_pipeline) = pipeline(EMPTY_PIPELINE);
    let explicit_path = override_pipeline
        .parent()
        .expect("pipeline path must have a parent")
        .join("chosen-trace.json");
    let explicit_arg = explicit_path
        .to_str()
        .expect("explicit trace path must be UTF-8");
    let explicit = run(&override_pipeline, &["--save-trace", explicit_arg]);
    assert!(explicit.status.success());
    assert!(explicit_path.is_file());
    assert!(trace_files(&override_pipeline).is_empty());
}

#[test]
fn success_modes_preserve_stream_and_exit_contracts() {
    let (_dir, path) = pipeline(EMPTY_PIPELINE);

    let default = run(&path, &[]);
    assert!(default.status.success());
    assert!(text(&default.stdout).contains("contract  PASS"));
    assert!(text(&default.stdout).contains("0/0 checks passed"));
    assert_ne!(text(&default.stdout), "null\n");
    assert_trace_saved(&default.stderr);

    let summary = run(&path, &["--summary"]);
    assert!(summary.status.success());
    assert!(text(&summary.stdout).contains("contract  PASS"));
    assert!(text(&summary.stdout).contains("0/0 checks passed"));
    assert_trace_saved(&summary.stderr);

    let verbose = run(&path, &["--verbose"]);
    assert!(verbose.status.success());
    assert!(text(&verbose.stdout).contains("Pipeline: contract"));
    assert!(text(&verbose.stdout).contains("Trace:"));
    assert_trace_saved(&verbose.stderr);

    let json = run(&path, &["--json"]);
    assert!(json.status.success());
    assert_eq!(text(&json.stdout), "null\n");
    assert_trace_saved(&json.stderr);

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
    assert_trace_saved(&verbose.stderr);
}

#[test]
fn budget_failure_has_one_diagnostic_and_exit_one() {
    let (_dir, path) = pipeline(
        "pipeline: budget-failure\nbudget: { steps: 0 }\nsteps:\n  - step: blocked\n    handler: ctrl::noop\n",
    );

    let output = run(&path, &[]);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert_trace_saved(&output.stderr);
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
    assert_trace_saved(&output.stderr);
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
