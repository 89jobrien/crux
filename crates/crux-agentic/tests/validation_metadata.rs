use crux_script::{HandlerRegistry, validate_pipeline};

#[test]
fn built_in_shell_metadata_validates_required_args() {
    let mut registry = HandlerRegistry::new();
    crux_agentic::register_all(&mut registry);

    let pipeline = crux_script::load(
        r#"
pipeline: valid
steps:
  - step: run
    handler: shell::capture
    args:
      cmd: echo hello
"#,
    )
    .unwrap();

    let report = validate_pipeline(&pipeline, &registry);
    assert!(report.is_ok(), "{:?}", report.diagnostics);
    assert!(registry.get_metadata("shell::capture").is_some());
}

#[test]
fn built_in_shell_metadata_reports_missing_cmd() {
    let mut registry = HandlerRegistry::new();
    crux_agentic::register_all(&mut registry);

    let pipeline = crux_script::load(
        r#"
pipeline: invalid
steps:
  - step: run
    handler: shell::capture
"#,
    )
    .unwrap();

    let report = validate_pipeline(&pipeline, &registry);
    assert_eq!(report.error_count(), 1);
    assert!(report.diagnostics[0].message.contains("cmd"));
}
