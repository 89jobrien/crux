use std::{collections::BTreeMap, str::FromStr as _, sync::Arc};

use chrono::{DateTime, Utc};
use crux_improve::{
    CandidateTracePolicy, Crux, EvalReport, EvalThresholds, LatencyComparison, LatencyRatio,
    RegressionEvaluation, RegressionPolicy, StructuralPolicy, TraceDiff, TraceDiffMode,
    TraceMetrics, Verdict,
};
use crux_regression::{
    BaselineRef, FileRegressionStore, InMemoryRegressionStore, RegressionCaseId, RegressionReport,
    RegressionRunId, RegressionStore, TraceDigest,
};
use serde_json::Value;

#[test]
fn in_memory_store_satisfies_contract() {
    assert_store_contract(InMemoryRegressionStore::default());
}

#[test]
fn in_memory_store_initial_cas_has_one_concurrent_winner() {
    let store = Arc::new(InMemoryRegressionStore::default());
    let case = RegressionCaseId::new("concurrent-cas").unwrap();
    let digest = store.put_trace(&sample_trace()).unwrap();
    let baseline = BaselineRef {
        case: case.clone(),
        trace: digest,
        labels: BTreeMap::new(),
        promoted_at: DateTime::UNIX_EPOCH,
    };
    let handles = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            let case = case.clone();
            let baseline = baseline.clone();
            std::thread::spawn(move || store.compare_and_set_baseline(&case, None, &baseline))
        })
        .collect::<Vec<_>>();

    let successes = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().is_ok())
        .filter(|succeeded| *succeeded)
        .count();

    assert_eq!(successes, 1);
}

#[test]
fn file_store_persists_objects_across_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let trace = sample_trace();
    let digest = FileRegressionStore::open(directory.path())
        .unwrap()
        .put_trace(&trace)
        .unwrap();

    let reopened = FileRegressionStore::open(directory.path()).unwrap();
    assert_eq!(
        serde_json::to_value(reopened.trace(&digest).unwrap()).unwrap(),
        serde_json::to_value(trace).unwrap()
    );
}

#[test]
fn file_store_satisfies_contract() {
    let directory = tempfile::tempdir().unwrap();
    assert_store_contract(FileRegressionStore::open(directory.path()).unwrap());
}

#[test]
fn file_store_creates_required_directories() {
    let directory = tempfile::tempdir().unwrap();
    FileRegressionStore::open(directory.path()).unwrap();

    for name in ["objects", "baselines", "reports", "locks"] {
        assert!(directory.path().join(name).is_dir(), "missing {name}");
    }
}

#[test]
fn file_store_rejects_corrupt_or_mismatched_envelopes() {
    let directory = tempfile::tempdir().unwrap();
    let store = FileRegressionStore::open(directory.path()).unwrap();
    let digest = store.put_trace(&sample_trace()).unwrap();
    let object = directory.path().join("objects").join(format!(
        "{}.json",
        digest.to_string().trim_start_matches("sha256:")
    ));

    std::fs::write(&object, b"{}").unwrap();
    assert!(store.trace(&digest).is_err());

    store.put_trace(&sample_trace()).unwrap_err();
    let mut envelope: serde_json::Value = serde_json::from_slice(
        &serde_json::to_vec(&serde_json::json!({
            "format_version": 1,
            "digest_algorithm": "sha256",
            "trace": sample_trace(),
        }))
        .unwrap(),
    )
    .unwrap();
    envelope["trace"]["agent"] = serde_json::json!("tampered");
    std::fs::write(&object, serde_json::to_vec(&envelope).unwrap()).unwrap();
    assert!(store.trace(&digest).is_err());
}

#[test]
fn file_store_initial_cas_has_one_concurrent_winner() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(FileRegressionStore::open(directory.path()).unwrap());
    let case = RegressionCaseId::new("file-concurrent-cas").unwrap();
    let digest = store.put_trace(&sample_trace()).unwrap();
    let baseline = BaselineRef {
        case: case.clone(),
        trace: digest,
        labels: BTreeMap::new(),
        promoted_at: DateTime::UNIX_EPOCH,
    };
    let handles = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            let case = case.clone();
            let baseline = baseline.clone();
            std::thread::spawn(move || store.compare_and_set_baseline(&case, None, &baseline))
        })
        .collect::<Vec<_>>();

    let successes = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().is_ok())
        .filter(|succeeded| *succeeded)
        .count();

    assert_eq!(successes, 1);
}

#[test]
fn file_store_baseline_and_reports_survive_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let store = FileRegressionStore::open(directory.path()).unwrap();
    let case = RegressionCaseId::new("reopen-history").unwrap();
    let digest = store.put_trace(&sample_trace()).unwrap();
    let baseline = BaselineRef {
        case: case.clone(),
        trace: digest.clone(),
        labels: BTreeMap::new(),
        promoted_at: DateTime::UNIX_EPOCH,
    };
    store
        .compare_and_set_baseline(&case, None, &baseline)
        .unwrap();
    let mut older = sample_report(case.clone(), digest.clone());
    older.id = RegressionRunId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    let mut newer = sample_report(case.clone(), digest);
    newer.id = RegressionRunId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap();
    store.append_report(&older).unwrap();
    store.append_report(&newer).unwrap();

    let reopened = FileRegressionStore::open(directory.path()).unwrap();

    assert_eq!(reopened.baseline(&case).unwrap(), baseline);
    assert_eq!(reopened.reports(&case).unwrap(), vec![newer, older]);
}

#[test]
fn file_store_rejects_truncated_baseline_and_report() {
    let directory = tempfile::tempdir().unwrap();
    let store = FileRegressionStore::open(directory.path()).unwrap();
    let case = RegressionCaseId::new("truncated-state").unwrap();
    let digest = store.put_trace(&sample_trace()).unwrap();
    let baseline = BaselineRef {
        case: case.clone(),
        trace: digest.clone(),
        labels: BTreeMap::new(),
        promoted_at: DateTime::UNIX_EPOCH,
    };
    store
        .compare_and_set_baseline(&case, None, &baseline)
        .unwrap();
    std::fs::write(
        directory
            .path()
            .join("baselines")
            .join(format!("{case}.json")),
        b"{",
    )
    .unwrap();
    assert!(store.baseline(&case).is_err());

    let report = sample_report(case.clone(), digest);
    store.append_report(&report).unwrap();
    std::fs::write(
        directory
            .path()
            .join("reports")
            .join(case.as_str())
            .join(format!("{}.json", report.id)),
        b"{",
    )
    .unwrap();
    assert!(store.reports(&case).is_err());
}

#[test]
fn file_store_rejects_semantically_misplaced_state() {
    let directory = tempfile::tempdir().unwrap();
    let store = FileRegressionStore::open(directory.path()).unwrap();
    let case = RegressionCaseId::new("misplaced-state").unwrap();
    let digest = store.put_trace(&sample_trace()).unwrap();
    let baseline = BaselineRef {
        case: case.clone(),
        trace: digest.clone(),
        labels: BTreeMap::new(),
        promoted_at: DateTime::UNIX_EPOCH,
    };
    store
        .compare_and_set_baseline(&case, None, &baseline)
        .unwrap();
    let baseline_path = directory
        .path()
        .join("baselines")
        .join(format!("{case}.json"));
    let mut baseline_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&baseline_path).unwrap()).unwrap();
    baseline_json["case"] = serde_json::json!("another-case");
    std::fs::write(&baseline_path, serde_json::to_vec(&baseline_json).unwrap()).unwrap();
    assert!(store.baseline(&case).is_err());

    let report = sample_report(case.clone(), digest);
    store.append_report(&report).unwrap();
    let report_path = directory
        .path()
        .join("reports")
        .join(case.as_str())
        .join(format!("{}.json", report.id));
    let mut report_json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
    report_json["id"] = serde_json::json!("01ARZ3NDEKTSV4RRFFQ69G5FAW");
    std::fs::write(&report_path, serde_json::to_vec(&report_json).unwrap()).unwrap();
    assert!(store.reports(&case).is_err());
}

#[cfg(unix)]
#[test]
fn file_store_rejects_managed_directory_symlinks() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("store")).unwrap();
    symlink(external.path(), directory.path().join("store/objects")).unwrap();

    assert!(FileRegressionStore::open(directory.path().join("store")).is_err());
}

fn assert_store_contract<S: RegressionStore>(store: S) {
    let case = RegressionCaseId::new("store-contract").unwrap();
    assert!(store.baseline(&case).is_err());
    let missing = TraceDigest::from_str(&format!("sha256:{}", "00".repeat(32))).unwrap();
    assert!(store.trace(&missing).is_err());
    assert!(store.reports(&case).unwrap().is_empty());
    let trace = sample_trace();
    let digest = store.put_trace(&trace).unwrap();
    assert_eq!(store.put_trace(&trace).unwrap(), digest);
    assert_eq!(
        serde_json::to_value(store.trace(&digest).unwrap()).unwrap(),
        serde_json::to_value(&trace).unwrap()
    );

    let baseline = BaselineRef {
        case: case.clone(),
        trace: digest.clone(),
        labels: BTreeMap::new(),
        promoted_at: DateTime::UNIX_EPOCH,
    };
    store
        .compare_and_set_baseline(&case, None, &baseline)
        .unwrap();
    assert_eq!(store.baseline(&case).unwrap(), baseline);

    let mut replacement_trace = sample_trace();
    replacement_trace.agent = "replacement".into();
    let replacement_digest = store.put_trace(&replacement_trace).unwrap();
    let replacement = BaselineRef {
        case: case.clone(),
        trace: replacement_digest,
        labels: BTreeMap::new(),
        promoted_at: DateTime::UNIX_EPOCH,
    };
    store
        .compare_and_set_baseline(&case, Some(&digest), &replacement)
        .unwrap();
    assert_eq!(store.baseline(&case).unwrap(), replacement);

    assert!(
        store
            .compare_and_set_baseline(&case, Some(&digest), &baseline)
            .is_err()
    );
    assert_eq!(store.baseline(&case).unwrap(), replacement);

    let report = sample_report(case, digest);
    store.append_report(&report).unwrap();
    assert!(store.append_report(&report).is_err());
    assert_eq!(store.reports(&report.case).unwrap(), vec![report]);
}

fn sample_trace() -> Crux<Value> {
    Crux {
        id: serde_json::from_str("\"crux_01ARZ3NDEKTSV4RRFFQ69G5FAV\"").unwrap(),
        agent: "store-test".into(),
        pipeline_version: None,
        value: Ok(serde_json::json!({"answer": 42})),
        steps: Vec::new(),
        children: Vec::new(),
        started_at: DateTime::UNIX_EPOCH,
        finished_at: Some(DateTime::UNIX_EPOCH),
    }
}

fn sample_report(case: RegressionCaseId, digest: TraceDigest) -> RegressionReport {
    let metrics = TraceMetrics {
        step_count: 0,
        success_rate: 0.0,
        error_count: 0,
        avg_confidence: 0.0,
        total_duration_ms: 0,
        delegation_count: 0,
        delegation_depth: 0,
        speculation_count: 0,
        speculation_hit_count: 0,
        speculation_hit_rate: 0.0,
        score: 0.0,
    };
    let comparison = crux_improve::Comparison {
        old_metrics: metrics.clone(),
        new_metrics: metrics,
        delta: 0.0,
        verdict: Verdict::Neutral,
    };
    let evaluation = RegressionEvaluation {
        passed: true,
        metrics: EvalReport {
            passed: true,
            comparison,
            latency_ratio: 1.0,
            latency: LatencyComparison {
                baseline_ms: 0,
                candidate_ms: 0,
                ratio: LatencyRatio::Finite(1.0),
            },
            failures: Vec::new(),
        },
        structure: TraceDiff {
            mode: TraceDiffMode::Strict,
            matches: true,
            step_diffs: Vec::new(),
        },
        invariant_violations: Vec::new(),
        failures: Vec::new(),
    };
    RegressionReport {
        id: RegressionRunId::new(),
        case,
        baseline: digest.clone(),
        candidate: digest,
        policy: RegressionPolicy {
            metrics: EvalThresholds {
                min_score_delta: 0.0,
                max_latency_ratio: 1.0,
            },
            structure: StructuralPolicy {
                mode: TraceDiffMode::Strict,
                confidence_tolerance: 0.0,
                compare_outputs: true,
                candidate_trace: CandidateTracePolicy::LiveOnly,
            },
            expected_output: None,
            invariants: Vec::new(),
        },
        evaluation,
        created_at: Utc::now(),
    }
}
