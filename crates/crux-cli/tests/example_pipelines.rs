//! Integration tests for example pipeline files.
//!
//! These tests verify that all example pipelines in `examples/` can be loaded,
//! have matching input fixture files, and reference only handlers that exist in
//! the real handler registry.

use crux_script::{HandlerRegistry, collect_handler_names, load_file};
use std::path::PathBuf;

fn example_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples")
}

/// Build the full handler registry that the CLI uses at runtime.
fn full_registry() -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();
    crux_agentic::register_all(&mut registry);
    registry
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
            .unwrap_or_else(|_| panic!("failed to read input_{}.json", example));
        serde_json::from_str::<serde_json::Value>(&contents)
            .unwrap_or_else(|_| panic!("input_{}.json should be valid JSON", example));
    }
}

/// Verify that every `handler:` reference in each example pipeline resolves to
/// a handler that is registered in the real runtime registry.
///
/// This test catches stale examples that reference non-existent handler names
/// before they reach a user. Pure YAML parsing is not sufficient; we must check
/// against the live registry.
#[test]
fn example_pipelines_reference_only_registered_handlers() {
    let registry = full_registry();
    let examples = ["core_harness", "ci_review", "deployment_triage", "handoff"];

    for example in examples {
        let path = example_dir().join(format!("{}.crux", example));
        let pipeline =
            load_file(&path).unwrap_or_else(|e| panic!("{}.crux failed to load: {}", example, e));

        let handler_names = collect_handler_names(&pipeline);
        let mut unknown = Vec::new();
        for name in &handler_names {
            if registry.runner(name).is_none() {
                unknown.push(name.clone());
            }
        }

        assert!(
            unknown.is_empty(),
            "{}.crux references handler(s) not in the registry: {:?}\n\
             All handlers in this pipeline: {:?}",
            example,
            unknown,
            handler_names,
        );
    }
}
