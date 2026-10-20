//! Integration tests for example pipeline files.
//!
//! These tests verify that all example pipelines in `examples/` can be loaded
//! and have matching input fixture files.

use crux_script::load_file;
use std::path::PathBuf;

fn example_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples")
}

#[test]
fn core_harness_pipeline_loads() {
    let path = example_dir().join("core_harness.crux");
    let result = load_file(&path);
    assert!(
        result.is_ok(),
        "core_harness.crux should load successfully: {:?}",
        result.err()
    );
    let pipeline = result.unwrap();
    assert_eq!(pipeline.pipeline, "core_harness");
}

#[test]
fn ci_review_pipeline_loads() {
    let path = example_dir().join("ci_review.crux");
    let result = load_file(&path);
    assert!(
        result.is_ok(),
        "ci_review.crux should load successfully: {:?}",
        result.err()
    );
    let pipeline = result.unwrap();
    assert_eq!(pipeline.pipeline, "ci_review");
}

#[test]
fn deployment_triage_pipeline_loads() {
    let path = example_dir().join("deployment_triage.crux");
    let result = load_file(&path);
    assert!(
        result.is_ok(),
        "deployment_triage.crux should load successfully: {:?}",
        result.err()
    );
    let pipeline = result.unwrap();
    assert_eq!(pipeline.pipeline, "deployment_triage");
}

#[test]
fn handoff_pipeline_loads() {
    let path = example_dir().join("handoff.crux");
    let result = load_file(&path);
    assert!(
        result.is_ok(),
        "handoff.crux should load successfully: {:?}",
        result.err()
    );
    let pipeline = result.unwrap();
    assert_eq!(pipeline.pipeline, "handoff");
}

#[test]
fn all_example_input_fixtures_exist() {
    let examples = vec!["core_harness", "ci_review", "deployment_triage", "handoff"];

    for example in examples {
        let input_path = example_dir().join(format!("input_{}.json", example));
        assert!(
            input_path.exists(),
            "input fixture input_{}.json should exist",
            example
        );
    }
}

#[test]
fn example_input_fixtures_are_valid_json() {
    let examples = vec!["core_harness", "ci_review", "deployment_triage", "handoff"];

    for example in examples {
        let input_path = example_dir().join(format!("input_{}.json", example));
        let contents = std::fs::read_to_string(&input_path)
            .expect(&format!("failed to read input_{}.json", example));
        serde_json::from_str::<serde_json::Value>(&contents)
            .expect(&format!("input_{}.json should be valid JSON", example));
    }
}
