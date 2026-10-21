use std::fs;

use crux_script::{load_file, schema::StepDef};
use tempfile::tempdir;

#[test]
fn load_file_inlines_relative_pipeline_steps() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("shared")).unwrap();
    fs::write(
        dir.path().join("shared/health.crux"),
        "pipeline: health\nsteps:\n  - step: check\n    handler: health\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("deploy.crux"),
        "pipeline: deploy\nsteps:\n  - include: shared/health.crux\n",
    )
    .unwrap();

    let pipeline = load_file(dir.path().join("deploy.crux")).unwrap();
    assert_eq!(pipeline.steps.len(), 1);
    assert!(matches!(&pipeline.steps[0], StepDef::Step(step) if step.step == "check"));
}

#[test]
fn load_file_rejects_include_cycles() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("a.crux"),
        "pipeline: a\nsteps:\n  - include: b.crux\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("b.crux"),
        "pipeline: b\nsteps:\n  - include: a.crux\n",
    )
    .unwrap();

    let error = load_file(dir.path().join("a.crux")).unwrap_err();
    assert!(error.to_string().contains("include cycle"), "{error}");
}
