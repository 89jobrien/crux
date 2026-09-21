//! Tests for compiler-backed validation diagnostics and metadata.

use crux_script::{
    ArgSchema, ArgType, CompileOptions, DiagnosticSeverity, HandlerMetadata, HandlerRegistry,
    RiskLevel, ValidationCode, compile_cruxfile, compile_pipeline, validate_cruxfile,
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

fn codes_and_locations(
    diagnostics: &[crux_script::ValidationDiagnostic],
) -> Vec<(ValidationCode, &str)> {
    diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code, diagnostic.location.as_str()))
        .collect()
}

#[test]
fn legacy_validation_uses_compiler_diagnostics() {
    let pipeline = crux_script::load(
        r#"
pipeline: duplicate
steps:
  - step: repeated
    handler: json::pick
  - step: repeated
    handler: json::pick
"#,
    )
    .unwrap();
    let registry = registry();
    let compiler = compile_pipeline(&pipeline, &registry, CompileOptions::permissive());
    let legacy = validate_pipeline(&pipeline, &registry);

    assert_eq!(
        codes_and_locations(&legacy.diagnostics),
        codes_and_locations(compiler.diagnostics())
    );

    let cruxfile = crux_script::load_cruxfile(
        r#"
project: duplicate
default: check
targets:
  check:
    steps:
      - step: repeated
        handler: json::pick
      - step: repeated
        handler: json::pick
"#,
    )
    .unwrap();
    let compiler = compile_cruxfile(&cruxfile, &registry, CompileOptions::permissive());
    let legacy = validate_cruxfile(&cruxfile, &registry);

    assert_eq!(
        codes_and_locations(&legacy.diagnostics),
        codes_and_locations(compiler.diagnostics())
    );
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
fn validation_accepts_compiler_success() {
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

    let report = validate_pipeline(&pipeline, &registry());
    assert!(report.is_ok(), "{:?}", report.diagnostics);
}

#[test]
fn validation_preserves_compiler_codes_and_locations() {
    let pipeline = crux_script::load(
        r#"
pipeline: invalid
steps:
  - step: missing
    handler: plugin::missing
"#,
    )
    .unwrap();

    let report = validate_pipeline(&pipeline, &registry());
    assert_eq!(report.error_count(), 0);
    assert_eq!(report.warning_count(), 1);
    assert_eq!(report.diagnostics[0].code, ValidationCode::UnknownHandler);
    assert_eq!(report.diagnostics[0].location, "steps[0]");
    assert_eq!(report.diagnostics[0].severity, DiagnosticSeverity::Warning);
}

#[test]
fn validation_reports_compiler_duplicate_name() {
    let pipeline = crux_script::load(
        r#"
pipeline: duplicate
steps:
  - step: read
    handler: json::pick
  - step: read
    handler: json::pick
"#,
    )
    .unwrap();

    let report = validate_pipeline(&pipeline, &registry());
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == ValidationCode::DuplicateName && diagnostic.location == "steps[1]"
    }));
}
