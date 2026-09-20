use crux_script::{
    ArgSchema, Compilation, CompileOptions, ConfidenceCapability, DiagnosticSeverity,
    HandlerMetadata, HandlerRegistry, ObjectSchema, ValidationCode, ValidationDiagnostic,
    ValueSchema, compile_pipeline,
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
    for (name, confidence) in [
        ("test::producer", ConfidenceCapability::Always),
        ("test::optional", ConfidenceCapability::Optional),
        ("test::never", ConfidenceCapability::Never),
    ] {
        registry.handler_value_with_metadata(
            HandlerMetadata::new(name)
                .args(ArgSchema::strict())
                .input_schema(ValueSchema::Dynamic)
                .output_schema(ValueSchema::object(
                    ObjectSchema::new().required("result", ValueSchema::String),
                ))
                .confidence(confidence),
            |input: Value| async move { Ok(input) },
        );
    }
    for (name, input, output, confidence) in [
        (
            "test::string_source",
            ValueSchema::Dynamic,
            ValueSchema::String,
            ConfidenceCapability::Always,
        ),
        (
            "test::string_to_integer",
            ValueSchema::String,
            ValueSchema::Integer,
            ConfidenceCapability::Never,
        ),
        (
            "test::integer_only",
            ValueSchema::Integer,
            ValueSchema::Integer,
            ConfidenceCapability::Never,
        ),
        (
            "test::join_string",
            ValueSchema::Integer,
            ValueSchema::String,
            ConfidenceCapability::Always,
        ),
        (
            "test::join_integer",
            ValueSchema::Integer,
            ValueSchema::Integer,
            ConfidenceCapability::Never,
        ),
    ] {
        registry.handler_value_with_metadata(
            HandlerMetadata::new(name)
                .args(ArgSchema::strict())
                .input_schema(input)
                .output_schema(output)
                .confidence(confidence),
            |input: Value| async move { Ok(input) },
        );
    }
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
input_schema:
  type: dynamic
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
input_schema:
  type: dynamic
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
input_schema:
  type: dynamic
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

#[test]
fn strict_compiler_requires_input_schema() {
    let pipeline = crux_script::load(
        r#"
pipeline: missing-input-schema
steps:
  - step: echo
    handler: test::echo
"#,
    )
    .unwrap();

    let compilation = compile_pipeline(&pipeline, &registry(), CompileOptions::strict());

    assert!(!compilation.is_ok());
    assert!(
        compilation
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == ValidationCode::MissingInputSchema)
    );
}

#[test]
fn compiler_preserves_bare_input_type() {
    let pipeline = crux_script::load(
        r#"
pipeline: typed-input
input_schema:
  type: string
steps:
  - step: echo
    handler: test::echo
    args:
      value: "{{ input }}"
"#,
    )
    .unwrap();

    let compilation = compile_pipeline(&pipeline, &registry(), CompileOptions::strict());
    let typed = compilation.artifact().unwrap();

    assert_eq!(typed.input_schema(), Some(&ValueSchema::String));
    assert_eq!(
        typed.step_argument_schema("echo", "value"),
        Some(&ValueSchema::String)
    );
}

#[test]
fn compiler_infers_variables_in_declaration_order() {
    let pipeline = crux_script::load(
        r#"
pipeline: typed-vars
input_schema:
  type: dynamic
vars:
  first: 3
  second: "{{ vars.first }}"
steps:
  - step: echo
    handler: test::echo
    args:
      value: "{{ vars.second }}"
"#,
    )
    .unwrap();

    let compilation = compile_pipeline(&pipeline, &registry(), CompileOptions::strict());
    let typed = compilation.artifact().unwrap();

    assert_eq!(typed.variable_schema("first"), Some(&ValueSchema::Integer));
    assert_eq!(typed.variable_schema("second"), Some(&ValueSchema::Integer));
    assert_eq!(
        typed.step_argument_schema("echo", "value"),
        Some(&ValueSchema::Integer)
    );
}

#[test]
fn compiler_rejects_forward_variable_reference() {
    let pipeline = crux_script::load(
        r#"
pipeline: forward-var
input_schema:
  type: dynamic
vars:
  second: "{{ vars.first }}"
  first: 3
steps:
  - step: echo
    handler: test::echo
"#,
    )
    .unwrap();

    let compilation = compile_pipeline(&pipeline, &registry(), CompileOptions::strict());

    assert!(!compilation.is_ok());
    assert!(
        compilation
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == ValidationCode::ForwardReference)
    );
}

#[test]
fn compiler_rejects_self_and_unknown_variable_references() {
    let self_reference = crux_script::load(
        r#"
pipeline: self-var
input_schema:
  type: dynamic
vars:
  value: "{{ vars.value }}"
steps: []
"#,
    )
    .unwrap();
    let unknown_reference = crux_script::load(
        r#"
pipeline: unknown-var
input_schema:
  type: dynamic
vars:
  value: "{{ vars.missing }}"
steps: []
"#,
    )
    .unwrap();

    let self_compilation = compile_pipeline(&self_reference, &registry(), CompileOptions::strict());
    let unknown_compilation =
        compile_pipeline(&unknown_reference, &registry(), CompileOptions::strict());

    assert!(
        self_compilation
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == ValidationCode::InvalidScope)
    );
    assert!(
        unknown_compilation
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == ValidationCode::UnknownReference)
    );
}

#[test]
fn compiler_checks_output_paths_and_confidence() {
    let pipeline = crux_script::load(
        r#"
pipeline: prior-step-paths
input_schema:
  type: dynamic
steps:
  - step: produce
    handler: test::producer
  - step: consume
    handler: test::echo
    args:
      value: "{{ steps.produce.output.result }}"
      score: "{{ steps.produce.confidence }}"
"#,
    )
    .unwrap();

    let compilation = compile_pipeline(&pipeline, &registry(), CompileOptions::strict());
    let typed = compilation.artifact().unwrap();

    assert_eq!(
        typed.step_argument_schema("consume", "value"),
        Some(&ValueSchema::String)
    );
    assert_eq!(
        typed.step_argument_schema("consume", "score"),
        Some(&ValueSchema::Number)
    );
}

#[test]
fn compiler_rejects_future_and_missing_step_paths() {
    let future = crux_script::load(
        r#"
pipeline: future-step
input_schema:
  type: dynamic
steps:
  - step: consume
    handler: test::echo
    args:
      value: "{{ steps.produce.output.result }}"
  - step: produce
    handler: test::producer
"#,
    )
    .unwrap();
    let missing = crux_script::load(
        r#"
pipeline: missing-path
input_schema:
  type: dynamic
steps:
  - step: produce
    handler: test::producer
  - step: consume
    handler: test::echo
    args:
      value: "{{ steps.produce.output.missing }}"
"#,
    )
    .unwrap();

    let future = compile_pipeline(&future, &registry(), CompileOptions::strict());
    let missing = compile_pipeline(&missing, &registry(), CompileOptions::strict());
    assert!(
        future
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == ValidationCode::ForwardReference)
    );
    assert!(
        missing
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == ValidationCode::UnknownReference)
    );
}

#[test]
fn compiler_checks_confidence_capability() {
    let never = crux_script::load(
        r#"
pipeline: no-confidence
input_schema:
  type: dynamic
steps:
  - step: produce
    handler: test::never
  - step: consume
    handler: test::echo
    args:
      score: "{{ steps.produce.confidence }}"
"#,
    )
    .unwrap();
    let optional = crux_script::load(
        r#"
pipeline: optional-confidence
steps:
  - step: produce
    handler: test::optional
  - step: consume
    handler: test::echo
    args:
      score: "{{ steps.produce.confidence }}"
"#,
    )
    .unwrap();

    let never = compile_pipeline(&never, &registry(), CompileOptions::strict());
    let optional = compile_pipeline(&optional, &registry(), CompileOptions::permissive());
    assert!(!never.is_ok());
    assert!(optional.is_executable());
    assert!(
        optional
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == ValidationCode::DynamicBoundary)
    );
}

#[test]
fn compiler_infers_pipe_and_join_outputs() {
    let pipeline = crux_script::load(
        r#"
pipeline: typed-combinators
input_schema:
  type: dynamic
steps:
  - pipe: transform
    stages:
      - step: source
        handler: test::string_source
      - step: length
        handler: test::string_to_integer
  - join_all: collect
    arms:
      - step: text
        handler: test::join_string
      - step: count
        handler: test::join_integer
"#,
    )
    .unwrap();

    let compilation = compile_pipeline(&pipeline, &registry(), CompileOptions::strict());
    let typed = compilation.artifact().unwrap();

    assert_eq!(
        typed.step_output_schema("transform"),
        Some(&ValueSchema::Integer)
    );
    assert_eq!(
        typed.step_output_schema("collect"),
        Some(&ValueSchema::array(
            ValueSchema::union([ValueSchema::String, ValueSchema::Integer]).unwrap()
        ))
    );
}

#[test]
fn compiler_rejects_incompatible_pipe_input() {
    let pipeline = crux_script::load(
        r#"
pipeline: incompatible-pipe
input_schema:
  type: dynamic
steps:
  - pipe: transform
    stages:
      - step: source
        handler: test::string_source
      - step: consume
        handler: test::integer_only
"#,
    )
    .unwrap();

    let compilation = compile_pipeline(&pipeline, &registry(), CompileOptions::strict());

    assert!(!compilation.is_ok());
    assert!(compilation.diagnostics().iter().any(|diagnostic| {
        diagnostic.code == ValidationCode::TypeMismatch
            && diagnostic.location == "steps[0].stages[1].input"
    }));
}

#[test]
fn compiler_infers_mixed_confidence_join() {
    let pipeline = crux_script::load(
        r#"
pipeline: mixed-confidence-join
input_schema:
  type: integer
steps:
  - join_all: collect
    arms:
      - step: text
        handler: test::join_string
      - step: count
        handler: test::join_integer
"#,
    )
    .unwrap();

    let compilation = compile_pipeline(&pipeline, &registry(), CompileOptions::strict());

    assert_eq!(
        compilation.artifact().unwrap().step_confidence("collect"),
        Some(ConfidenceCapability::Optional)
    );
}
