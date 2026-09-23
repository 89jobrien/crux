use std::{path::Path, process::Command};

fn crux() -> Command {
    Command::new(env!("CARGO_BIN_EXE_crux"))
}

fn fixture_trace() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fixtures/showcase_trace.json")
}

fn ingest(store: &Path, trace: &Path) -> String {
    let output = crux()
        .args(["regress", "ingest"])
        .arg(trace)
        .arg("--store")
        .arg(store)
        .output()
        .expect("ingest must execute");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[test]
fn regress_ingest_prints_digest_and_persists_trace() {
    let directory = tempfile::tempdir().unwrap();
    let digest = ingest(directory.path(), &fixture_trace());

    assert!(digest.starts_with("sha256:"), "{digest}");
    assert!(
        directory
            .path()
            .join("objects")
            .join(format!("{}.json", digest.trim_start_matches("sha256:")))
            .is_file()
    );
}

#[test]
fn regress_promote_creates_initial_baseline_and_guards_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let digest = ingest(directory.path(), &fixture_trace());
    let output = crux()
        .args(["regress", "promote", "showcase", &digest])
        .arg("--label")
        .arg("model=fixture")
        .arg("--store")
        .arg(directory.path())
        .output()
        .expect("promote must execute");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stale = crux()
        .args(["regress", "promote", "showcase", &digest])
        .arg("--store")
        .arg(directory.path())
        .output()
        .expect("stale promote must execute");
    assert!(!stale.status.success());
    assert_eq!(stale.status.code(), Some(1));
}

#[test]
fn regress_rejects_invalid_case_and_digest() {
    let directory = tempfile::tempdir().unwrap();
    let output = crux()
        .args(["regress", "promote", "../escape", "not-a-digest"])
        .arg("--store")
        .arg(directory.path())
        .output()
        .expect("invalid promote must execute");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn regress_evaluate_and_history_emit_machine_readable_reports() {
    let directory = tempfile::tempdir().unwrap();
    let policy = write_policy(directory.path());
    let digest = ingest(directory.path(), &fixture_trace());
    promote(directory.path(), "showcase", &digest);

    let evaluation = crux()
        .args(["regress", "evaluate", "showcase"])
        .arg(fixture_trace())
        .arg("--policy")
        .arg(&policy)
        .arg("--json")
        .arg("--store")
        .arg(directory.path())
        .output()
        .expect("evaluate must execute");
    assert!(
        evaluation.status.success(),
        "{}",
        String::from_utf8_lossy(&evaluation.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&evaluation.stdout).unwrap();
    assert_eq!(report["case"], "showcase");
    assert_eq!(report["evaluation"]["passed"], true);

    let history = crux()
        .args(["regress", "history", "showcase", "--json"])
        .arg("--store")
        .arg(directory.path())
        .output()
        .expect("history must execute");
    assert!(history.status.success());
    let reports: serde_json::Value = serde_json::from_slice(&history.stdout).unwrap();
    assert_eq!(reports.as_array().unwrap().len(), 1);
}

#[test]
fn regress_evaluate_failure_persists_report_and_exits_nonzero() {
    let directory = tempfile::tempdir().unwrap();
    let policy = write_policy(directory.path());
    let digest = ingest(directory.path(), &fixture_trace());
    promote(directory.path(), "showcase", &digest);
    let candidate = directory.path().join("candidate.json");
    let mut trace: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture_trace()).unwrap()).unwrap();
    trace["steps"][0]["status"] = serde_json::json!("err");
    std::fs::write(&candidate, serde_json::to_vec(&trace).unwrap()).unwrap();

    let evaluation = crux()
        .args(["regress", "evaluate", "showcase"])
        .arg(&candidate)
        .arg("--policy")
        .arg(&policy)
        .arg("--json")
        .arg("--store")
        .arg(directory.path())
        .output()
        .expect("evaluate must execute");

    assert!(!evaluation.status.success());
    assert_eq!(evaluation.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&evaluation.stdout).unwrap();
    assert_eq!(report["evaluation"]["passed"], false);
    assert!(directory.path().join("reports/showcase").is_dir());
}

#[test]
fn regress_diff_emits_structural_json_without_history() {
    let directory = tempfile::tempdir().unwrap();
    let policy = write_policy(directory.path());
    let digest = ingest(directory.path(), &fixture_trace());
    promote(directory.path(), "showcase", &digest);

    let output = crux()
        .args(["regress", "diff", "showcase"])
        .arg(fixture_trace())
        .arg("--policy")
        .arg(&policy)
        .arg("--json")
        .arg("--store")
        .arg(directory.path())
        .output()
        .expect("diff must execute");

    assert!(output.status.success());
    let diff: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(diff["matches"], true);
    assert!(!directory.path().join("reports/showcase").exists());
}

#[test]
fn regress_diff_mismatch_exits_two() {
    let directory = tempfile::tempdir().unwrap();
    let policy = write_policy(directory.path());
    let digest = ingest(directory.path(), &fixture_trace());
    promote(directory.path(), "showcase", &digest);
    let candidate = directory.path().join("candidate.json");
    let mut trace: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture_trace()).unwrap()).unwrap();
    trace["steps"][0]["status"] = serde_json::json!("err");
    std::fs::write(&candidate, serde_json::to_vec(&trace).unwrap()).unwrap();

    let output = crux()
        .args(["regress", "diff", "showcase"])
        .arg(candidate)
        .arg("--policy")
        .arg(policy)
        .arg("--json")
        .arg("--store")
        .arg(directory.path())
        .output()
        .expect("diff must execute");

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn regress_history_without_baseline_returns_empty_list() {
    let directory = tempfile::tempdir().unwrap();
    let output = crux()
        .args(["regress", "history", "missing", "--json"])
        .arg("--store")
        .arg(directory.path())
        .output()
        .expect("history must execute");

    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!([])
    );
}

#[test]
fn nightly_fixture_command_is_credential_free_and_passing() {
    let directory = tempfile::tempdir().unwrap();
    let fixture_root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fixtures/regression");
    let baseline = fixture_root.join("trace.json");
    let candidate = fixture_root.join("candidate.json");
    let policy = fixture_root.join("policy.json");
    let digest = ingest(directory.path(), &baseline);
    promote(directory.path(), "nightly-showcase", &digest);

    let output = crux()
        .args(["regress", "evaluate", "nightly-showcase"])
        .arg(candidate)
        .arg("--policy")
        .arg(policy)
        .arg("--json")
        .arg("--store")
        .arg(directory.path())
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .expect("nightly evaluation must execute");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["evaluation"]["passed"], true);
}

fn promote(store: &Path, case: &str, digest: &str) {
    let output = crux()
        .args(["regress", "promote", case, digest])
        .arg("--store")
        .arg(store)
        .output()
        .expect("promote must execute");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_policy(directory: &Path) -> std::path::PathBuf {
    let path = directory.join("policy.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "metrics": {
                "min_score_delta": 0.0,
                "max_latency_ratio": 1.0
            },
            "structure": {
                "mode": "strict",
                "confidence_tolerance": 0.0,
                "compare_outputs": true,
                "candidate_trace": "live_only"
            },
            "expected_output": null,
            "invariants": []
        }))
        .unwrap(),
    )
    .unwrap();
    path
}
