use crux_script::{
    ArgSchema, Compilation, CompileOptions, ConfidenceCapability, DiagnosticSeverity,
    HandlerMetadata, HandlerRegistry, ValidationCode, ValidationDiagnostic, ValueSchema,
    compile_pipeline,
};
use serde_json::Value;

fn registry() -> HandlerRegistry {
    let mut registry = HandlerRegistry::new();
    registry.handler_value_with_metadata(
        HandlerMetadata::new("test::echo")
            .args(ArgSchema::strict())
            .input_schema(ValueSchema::Dynamic)
            .output_schema(ValueSchema::Dynamic)
            .confidence(ConfidenceCapability::Never),
        |input: Value| async move { Ok(input) },
    );
    registry
}

#[test]
fn compilation_artifact_absent_on_error() {
    let compilation = Compilation::new(
        Some("typed-pipeline"),
        vec![ValidationDiagnostic::error_with_code(
            ValidationCode::TypeMismatch,
            "steps[0].args.limit",
            "expected integer, got string",
        )],
    );

    assert!(compilation.artifact().is_none());
    assert!(!compilation.is_ok());
    assert!(!compilation.is_executable());
    assert_eq!(compilation.error_count(), 1);
}

#[test]
fn compilation_counts_warnings() {
    let compilation = Compilation::new(
        Some("typed-pipeline"),
        vec![ValidationDiagnostic::warning_with_code(
            ValidationCode::DynamicBoundary,
            "steps[0].output",
            "output schema is dynamic",
        )],
    );

    assert_eq!(compilation.artifact(), Some(&"typed-pipeline"));
    assert!(compilation.is_ok());
    assert!(compilation.is_executable());
    assert_eq!(compilation.warning_count(), 1);
}

#[test]
fn strict_mode_escalates_dynamic_boundaries() {
    assert_eq!(
        CompileOptions::permissive().severity_for(ValidationCode::DynamicBoundary),
        DiagnosticSeverity::Warning
    );
    assert_eq!(
        CompileOptions::strict().severity_for(ValidationCode::DynamicBoundary),
        DiagnosticSeverity::Error
    );
    assert_eq!(
        CompileOptions::permissive().severity_for(ValidationCode::TypeMismatch),
        DiagnosticSeverity::Error
    );
}

#[test]
fn compiler_resolves_simple_handler() {
    let pipeline = crux_script::load(
        r#"
pipeline: typed-simple
steps:
  - step: echo
    handler: test::echo
"#,
    )
    .unwrap();

    let compilation = compile_pipeline(&pipeline, &registry(), CompileOptions::strict());

    assert!(compilation.is_executable());
    let typed = compilation.artifact().unwrap();
    assert_eq!(typed.name(), "typed-simple");
    assert_eq!(typed.step_count(), 1);
}

#[test]
fn compiler_rejects_unknown_handler_in_strict_mode() {
    let pipeline = crux_script::load(
        r#"
pipeline: strict-unknown
steps:
  - step: missing
    handler: plugin::missing
"#,
    )
    .unwrap();

    let compilation = compile_pipeline(&pipeline, &registry(), CompileOptions::strict());

    assert!(!compilation.is_ok());
    assert!(!compilation.is_executable());
    assert_eq!(
        compilation.diagnostics()[0].code,
        ValidationCode::UnknownHandler
    );
    assert_eq!(
        compilation.diagnostics()[0].severity,
        DiagnosticSeverity::Error
    );
}

#[test]
fn permissive_unknown_handler_is_not_executable() {
    let pipeline = crux_script::load(
        r#"
pipeline: permissive-unknown
steps:
  - step: missing
    handler: plugin::missing
"#,
    )
    .unwrap();

    let compilation = compile_pipeline(&pipeline, &registry(), CompileOptions::permissive());

    assert!(compilation.is_ok());
    assert!(!compilation.is_executable());
    assert_eq!(compilation.warning_count(), 1);
    assert_eq!(
        compilation.diagnostics()[0].code,
        ValidationCode::UnknownHandler
    );
}

fn expression_pipeline() -> crux_script::schema::PipelineDef {
    crux_script::load(
        r#"
pipeline: typed-expressions
steps:
  - step: echo
    handler: test::echo
    args:
      exact: "{{ input }}"
      message: "hello {{ input.name }}"
      count: 3
"#,
    )
    .unwrap()
}

#[test]
fn compiler_types_template_expressions() {
    let compilation = compile_pipeline(
        &expression_pipeline(),
        &registry(),
        CompileOptions::strict(),
    );
    let typed = compilation.artifact().unwrap();

    assert_eq!(
        typed.step_argument_schema("echo", "exact"),
        Some(&ValueSchema::Dynamic)
    );
    assert_eq!(
        typed.step_argument_schema("echo", "message"),
        Some(&ValueSchema::String)
    );
    assert_eq!(
        typed.step_argument_schema("echo", "count"),
        Some(&ValueSchema::Integer)
    );
}

#[test]
fn exact_template_preserves_type() {
    let compilation = compile_pipeline(
        &expression_pipeline(),
        &registry(),
        CompileOptions::strict(),
    );

    assert_eq!(
        compilation
            .artifact()
            .unwrap()
            .step_argument_schema("echo", "exact"),
        Some(&ValueSchema::Dynamic)
    );
}

#[test]
fn interpolation_produces_string() {
    let compilation = compile_pipeline(
        &expression_pipeline(),
        &registry(),
        CompileOptions::strict(),
    );

    assert_eq!(
        compilation
            .artifact()
            .unwrap()
            .step_argument_schema("echo", "message"),
        Some(&ValueSchema::String)
    );
}
