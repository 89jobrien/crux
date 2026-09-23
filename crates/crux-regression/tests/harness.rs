use std::collections::BTreeMap;

use chrono::DateTime;
use crux_improve::{
    CandidateTracePolicy, Crux, EvalThresholds, RegressionPolicy, StructuralPolicy, TraceDiffMode,
};
use crux_regression::{InMemoryRegressionStore, RegressionCaseId, RegressionHarness};
use serde_json::Value;

#[test]
fn harness_evaluates_and_persists_passing_report() {
    let harness = RegressionHarness::new(InMemoryRegressionStore::default());
    let case = RegressionCaseId::new("passing").unwrap();
    let trace = sample_trace(42);
    let digest = harness.ingest(&trace).unwrap();
    harness
        .promote(
            case.clone(),
            digest,
            None,
            BTreeMap::from([("model".into(), "fixture".into())]),
        )
        .unwrap();

    let report = harness.evaluate(&case, &trace, &policy(42)).unwrap();

    assert!(report.evaluation.passed);
    assert_eq!(harness.history(&case).unwrap(), vec![report]);
}

#[test]
fn harness_persists_failing_report_before_returning() {
    let harness = RegressionHarness::new(InMemoryRegressionStore::default());
    let case = RegressionCaseId::new("failing").unwrap();
    let baseline = sample_trace(42);
    let digest = harness.ingest(&baseline).unwrap();
    harness
        .promote(case.clone(), digest, None, BTreeMap::new())
        .unwrap();

    let report = harness
        .evaluate(&case, &sample_trace(41), &policy(42))
        .unwrap();

    assert!(!report.evaluation.passed);
    assert_eq!(harness.history(&case).unwrap().len(), 1);
}

#[test]
fn harness_requires_expected_digest_for_replacement() {
    let harness = RegressionHarness::new(InMemoryRegressionStore::default());
    let case = RegressionCaseId::new("replacement").unwrap();
    let first = harness.ingest(&sample_trace(42)).unwrap();
    let second = harness.ingest(&sample_trace(43)).unwrap();
    harness
        .promote(case.clone(), first.clone(), None, BTreeMap::new())
        .unwrap();

    assert!(
        harness
            .promote(case.clone(), second.clone(), None, BTreeMap::new())
            .is_err()
    );
    let promoted = harness
        .promote(case, second.clone(), Some(first), BTreeMap::new())
        .unwrap();
    assert_eq!(promoted.trace, second);
}

#[test]
fn harness_diff_is_read_only() {
    let harness = RegressionHarness::new(InMemoryRegressionStore::default());
    let case = RegressionCaseId::new("read-only-diff").unwrap();
    let baseline = sample_trace(42);
    let digest = harness.ingest(&baseline).unwrap();
    harness
        .promote(case.clone(), digest, None, BTreeMap::new())
        .unwrap();

    let diff = harness
        .diff(&case, &sample_trace(41), &policy(42).structure)
        .unwrap();

    assert!(diff.matches);
    assert!(harness.history(&case).unwrap().is_empty());
}

fn policy(answer: i64) -> RegressionPolicy {
    RegressionPolicy {
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
        expected_output: Some(serde_json::json!({"answer": answer})),
        invariants: Vec::new(),
    }
}

fn sample_trace(answer: i64) -> Crux<Value> {
    Crux {
        id: serde_json::from_str("\"crux_01ARZ3NDEKTSV4RRFFQ69G5FAV\"").unwrap(),
        agent: "harness-test".into(),
        pipeline_version: None,
        value: Ok(serde_json::json!({"answer": answer})),
        steps: Vec::new(),
        children: Vec::new(),
        started_at: DateTime::UNIX_EPOCH,
        finished_at: Some(DateTime::UNIX_EPOCH),
    }
}
