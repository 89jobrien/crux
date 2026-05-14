use cruxx_script::{
    ArgSchema, ArgType, DiagnosticSeverity, HandlerMetadata, HandlerRegistry, RiskLevel,
    validate_pipeline,
};
use serde_json::Value;

fn registry() -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();
    registry.handler_value_with_metadata(
        HandlerMetadata::new("shell::capture")
            .describe("test shell handler")
            .args(
                ArgSchema::new()
                    .required("cmd", ArgType::String)
                    .optional("cwd", ArgType::String),
            )
            .risk(RiskLevel::High),
        |input: Value| async move { Ok(input) },
    );
    registry.handler_value_with_metadata(
        HandlerMetadata::new("json::pick")
            .args(ArgSchema::new().optional("fields", ArgType::Array)),
        |input: Value| async move { Ok(input) },
    );
    registry
}

#[test]
fn metadata_is_registered_with_handler() {
    let registry = registry();
    let metadata = registry
        .get_metadata("shell::capture")
        .expect("metadata should exist");

    assert_eq!(metadata.name, "shell::capture");
    assert_eq!(metadata.risk, RiskLevel::High);
    assert!(metadata.args.get("cmd").is_some());
}

#[test]
fn validation_accepts_known_handler_with_required_args() {
    let pipeline = cruxx_script::load(
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

    let report = validate_pipeline(&pipeline, &registry());
    assert!(report.is_ok(), "{:?}", report.diagnostics);
}

#[test]
fn validation_reports_unknown_handler() {
    let pipeline = cruxx_script::load(
        r#"
pipeline: invalid
steps:
  - step: missing
    handler: unknown::handler
"#,
    )
    .unwrap();

    let report = validate_pipeline(&pipeline, &registry());
    assert_eq!(report.error_count(), 1);
    assert_eq!(report.diagnostics[0].severity, DiagnosticSeverity::Error);
    assert!(report.diagnostics[0].message.contains("not registered"));
}

#[test]
fn validation_reports_missing_required_args() {
    let pipeline = cruxx_script::load(
        r#"
pipeline: invalid
steps:
  - step: run
    handler: shell::capture
"#,
    )
    .unwrap();

    let report = validate_pipeline(&pipeline, &registry());
    assert_eq!(report.error_count(), 1);
    assert!(report.diagnostics[0].message.contains("cmd"));
}

#[test]
fn validation_reports_wrong_arg_type() {
    let pipeline = cruxx_script::load(
        r#"
pipeline: invalid
steps:
  - step: run
    handler: shell::capture
    args:
      cmd: ["echo", "hello"]
"#,
    )
    .unwrap();

    let report = validate_pipeline(&pipeline, &registry());
    assert_eq!(report.error_count(), 1);
    assert!(report.diagnostics[0].message.contains("expected string"));
}

#[test]
fn validation_allows_templated_static_args() {
    let pipeline = cruxx_script::load(
        r#"
pipeline: valid
steps:
  - step: run
    handler: shell::capture
    args:
      cmd: "{{ input.command }}"
"#,
    )
    .unwrap();

    let report = validate_pipeline(&pipeline, &registry());
    assert!(report.is_ok(), "{:?}", report.diagnostics);
}

#[test]
fn validation_checks_nested_handler_positions() {
    let pipeline = cruxx_script::load(
        r#"
pipeline: invalid
steps:
  - join_all: fetch
    arms:
      - step: a
        handler: json::pick
        args:
          fields: [name]
      - missing::handler
"#,
    )
    .unwrap();

    let report = validate_pipeline(&pipeline, &registry());
    assert_eq!(report.error_count(), 1);
    assert!(report.diagnostics[0].location.contains("arms[1]"));
}

#[test]
fn validation_reports_overlapping_confidence_routes() {
    let pipeline = cruxx_script::load(
        r#"
pipeline: invalid
steps:
  - route_on_confidence: route
    value: "{{ steps.analyze.confidence }}"
    routes:
      - range: "[0.0, 0.8]"
        label: low
        handler: json::pick
      - range: "[0.8, 1.0]"
        label: high
        handler: json::pick
"#,
    )
    .unwrap();

    let report = validate_pipeline(&pipeline, &registry());
    assert_eq!(report.error_count(), 1);
    assert!(report.diagnostics[0].message.contains("overlap"));
}
