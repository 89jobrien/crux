use std::sync::{Arc, Mutex};

use crux_runtime::prelude::CruxErr;
use crux_script::{
    ArgSchema, CompileOptions, ConfidenceCapability, HandlerExecution, HandlerMetadata,
    HandlerOutput, HandlerRegistry, Runner, StepFuture, StepInvocation, StepRunner, ValueSchema,
    compile_pipeline,
};
use serde_json::json;

struct RecordingRunner {
    metadata: HandlerMetadata,
    invocation: Arc<Mutex<Option<StepInvocation>>>,
}

struct ContractRunner {
    metadata: HandlerMetadata,
    output: HandlerOutput,
}

struct InvocationLogRunner {
    metadata: HandlerMetadata,
    invocations: Arc<Mutex<Vec<StepInvocation>>>,
}

impl StepRunner for ContractRunner {
    fn metadata(&self) -> &HandlerMetadata {
        &self.metadata
    }

    fn run(&self, _invocation: StepInvocation) -> StepFuture<'_> {
        let output = self.output.clone();
        Box::pin(async move { HandlerExecution::free(Ok(output)) })
    }
}

impl StepRunner for InvocationLogRunner {
    fn metadata(&self) -> &HandlerMetadata {
        &self.metadata
    }

    fn run(&self, invocation: StepInvocation) -> StepFuture<'_> {
        let invocations = Arc::clone(&self.invocations);
        Box::pin(async move {
            invocations.lock().unwrap().push(invocation);
            HandlerExecution::free(Ok(HandlerOutput::new(json!(null))))
        })
    }
}

async fn contract_failure(
    metadata: HandlerMetadata,
    output: HandlerOutput,
    input_schema: &str,
    args: &str,
    input: serde_json::Value,
) -> (String, String) {
    let mut registry = HandlerRegistry::new();
    registry
        .register(ContractRunner { metadata, output })
        .unwrap();
    let source = format!(
        r#"
pipeline: runtime-contracts
input_schema:
  type: {input_schema}
steps:
  - step: contract
    handler: test::contract
{args}
"#
    );
    let pipeline = crux_script::load(&source).unwrap();
    let compiled = compile_pipeline(&pipeline, &registry, CompileOptions::strict())
        .into_artifact()
        .unwrap();
    let result = Runner::new(Arc::new(HandlerRegistry::new()))
        .run_compiled(&compiled, input)
        .await;

    match result.value().unwrap_err() {
        CruxErr::StepFailed { step, source_msg } => (step.clone(), source_msg.clone()),
        error => panic!("expected StepFailed, got {error}"),
    }
}

impl StepRunner for RecordingRunner {
    fn metadata(&self) -> &HandlerMetadata {
        &self.metadata
    }

    fn run(&self, invocation: StepInvocation) -> StepFuture<'_> {
        let recorded = Arc::clone(&self.invocation);
        Box::pin(async move {
            *recorded.lock().unwrap() = Some(invocation);
            HandlerExecution::free(Ok(HandlerOutput::new(json!({"executed": true}))))
        })
    }
}

#[tokio::test]
async fn run_compiled_executes_resolved_runner() {
    let invocation = Arc::new(Mutex::new(None));
    let mut compile_registry = HandlerRegistry::new();
    compile_registry
        .register(RecordingRunner {
            metadata: HandlerMetadata::new("test::record")
                .args(ArgSchema::strict().required("source", ValueSchema::String))
                .input_schema(ValueSchema::Dynamic)
                .output_schema(ValueSchema::Dynamic)
                .confidence(ConfidenceCapability::Never),
            invocation: Arc::clone(&invocation),
        })
        .unwrap();
    let pipeline = crux_script::load(
        r#"
pipeline: typed-runner
input_schema:
  type: dynamic
steps:
  - step: record
    handler: test::record
    args:
      source: "{{ input.source }}"
"#,
    )
    .unwrap();
    let compiled = compile_pipeline(&pipeline, &compile_registry, CompileOptions::strict())
        .into_artifact()
        .unwrap();

    // The execution registry is intentionally empty: the compiled artifact must
    // retain the resolved runner rather than perform another name lookup.
    let result = Runner::new(Arc::new(HandlerRegistry::new()))
        .run_compiled(&compiled, json!({"source": "compiled"}))
        .await;

    assert_eq!(result.value().unwrap(), &json!({"executed": true}));
    let invocation = invocation.lock().unwrap();
    let invocation = invocation.as_ref().unwrap();
    assert_eq!(invocation.input(), &json!({"source": "compiled"}));
    assert_eq!(invocation.args(), &json!({"source": "compiled"}));
}

#[tokio::test]
async fn run_compiled_rejects_contract_violations_pipeline_input_mismatch() {
    let (step, message) = contract_failure(
        HandlerMetadata::new("test::contract")
            .input_schema(ValueSchema::Dynamic)
            .output_schema(ValueSchema::Integer)
            .confidence(ConfidenceCapability::Never),
        HandlerOutput::new(json!(1)),
        "string",
        "",
        json!(42),
    )
    .await;

    assert_eq!(step, "runtime-contracts");
    assert!(message.contains("pipeline input"));
    assert!(message.contains("expected string"));
}

#[tokio::test]
async fn run_compiled_rejects_contract_violations_handler_input_mismatch() {
    let (step, message) = contract_failure(
        HandlerMetadata::new("test::contract")
            .input_schema(ValueSchema::String)
            .output_schema(ValueSchema::Integer)
            .confidence(ConfidenceCapability::Never),
        HandlerOutput::new(json!(1)),
        "dynamic",
        "",
        json!(42),
    )
    .await;

    assert_eq!(step, "contract");
    assert!(message.contains("handler 'test::contract' input"));
    assert!(message.contains("expected string"));
}

#[tokio::test]
async fn run_compiled_rejects_contract_violations_argument_mismatch() {
    let (step, message) = contract_failure(
        HandlerMetadata::new("test::contract")
            .args(ArgSchema::strict().required("count", ValueSchema::Integer))
            .input_schema(ValueSchema::Dynamic)
            .output_schema(ValueSchema::Integer)
            .confidence(ConfidenceCapability::Never),
        HandlerOutput::new(json!(1)),
        "dynamic",
        "    args:\n      count: wrong",
        json!(null),
    )
    .await;

    assert_eq!(step, "contract");
    assert!(message.contains("handler 'test::contract' arguments"));
    assert!(message.contains("$.count"));
    assert!(message.contains("expected integer"));
}

#[tokio::test]
async fn run_compiled_rejects_contract_violations_output_mismatch() {
    let (step, message) = contract_failure(
        HandlerMetadata::new("test::contract")
            .input_schema(ValueSchema::Dynamic)
            .output_schema(ValueSchema::Integer)
            .confidence(ConfidenceCapability::Never),
        HandlerOutput::new(json!("wrong")),
        "dynamic",
        "",
        json!(null),
    )
    .await;

    assert_eq!(step, "contract");
    assert!(message.contains("handler 'test::contract' output"));
    assert!(message.contains("expected integer"));
}

#[tokio::test]
async fn run_compiled_rejects_contract_violations_always_without_confidence() {
    let (step, message) = contract_failure(
        HandlerMetadata::new("test::contract")
            .input_schema(ValueSchema::Dynamic)
            .output_schema(ValueSchema::Integer)
            .confidence(ConfidenceCapability::Always),
        HandlerOutput::new(json!(1)),
        "dynamic",
        "",
        json!(null),
    )
    .await;

    assert_eq!(step, "contract");
    assert!(message.contains("handler 'test::contract' confidence"));
    assert!(message.contains("always report confidence"));
}

#[tokio::test]
async fn run_compiled_rejects_contract_violations_never_with_confidence() {
    let (step, message) = contract_failure(
        HandlerMetadata::new("test::contract")
            .input_schema(ValueSchema::Dynamic)
            .output_schema(ValueSchema::Integer)
            .confidence(ConfidenceCapability::Never),
        HandlerOutput::with_confidence(json!(1), 0.8),
        "dynamic",
        "",
        json!(null),
    )
    .await;

    assert_eq!(step, "contract");
    assert!(message.contains("handler 'test::contract' confidence"));
    assert!(message.contains("never report confidence"));
}

#[tokio::test]
async fn loop_bindings_do_not_leak_at_runtime() {
    let invocations = Arc::new(Mutex::new(Vec::new()));
    let mut registry = HandlerRegistry::new();
    registry
        .register(InvocationLogRunner {
            metadata: HandlerMetadata::new("test::record")
                .args(
                    ArgSchema::strict()
                        .required("scope", ValueSchema::String)
                        .required("value", ValueSchema::String),
                )
                .input_schema(ValueSchema::Dynamic)
                .output_schema(ValueSchema::Null)
                .confidence(ConfidenceCapability::Never),
            invocations: Arc::clone(&invocations),
        })
        .unwrap();
    let pipeline = crux_script::load(
        r#"
pipeline: lexical-loop-scopes
input_schema:
  type: dynamic
vars:
  outer_items: [first, second]
  inner_items: [nested]
steps:
  - for_each: outer as item
    items: "{{ vars.outer_items }}"
    steps:
      - step: outer-before
        handler: test::record
        args:
          scope: outer-before
          value: "{{ iter.item }}"
      - for_each: inner as item
        items: "{{ vars.inner_items }}"
        steps:
          - step: inner-value
            handler: test::record
            args:
              scope: inner
              value: "{{ iter.item }}"
      - step: outer-after
        handler: test::record
        args:
          scope: outer-after
          value: "{{ iter.item }}"
"#,
    )
    .unwrap();
    let compiled = compile_pipeline(&pipeline, &registry, CompileOptions::strict())
        .into_artifact()
        .unwrap();

    let result = Runner::new(Arc::new(HandlerRegistry::new()))
        .run_compiled(&compiled, json!(null))
        .await;

    assert_eq!(result.value().unwrap(), &json!(null));
    let recorded = invocations
        .lock()
        .unwrap()
        .iter()
        .map(|invocation| invocation.args().clone())
        .collect::<Vec<_>>();
    assert_eq!(
        recorded,
        vec![
            json!({"scope": "outer-before", "value": "first"}),
            json!({"scope": "inner", "value": "nested"}),
            json!({"scope": "outer-after", "value": "first"}),
            json!({"scope": "outer-before", "value": "second"}),
            json!({"scope": "inner", "value": "nested"}),
            json!({"scope": "outer-after", "value": "second"}),
        ]
    );
}

#[tokio::test]
async fn repeated_iteration_binding_isolation() {
    let invocations = Arc::new(Mutex::new(Vec::new()));
    let mut registry = HandlerRegistry::new();
    registry
        .register(InvocationLogRunner {
            metadata: HandlerMetadata::new("test::record")
                .args(
                    ArgSchema::strict()
                        .required("index", ValueSchema::Integer)
                        .required("value", ValueSchema::String),
                )
                .input_schema(ValueSchema::Dynamic)
                .output_schema(ValueSchema::Null)
                .confidence(ConfidenceCapability::Never),
            invocations: Arc::clone(&invocations),
        })
        .unwrap();
    let pipeline = crux_script::load(
        r#"
pipeline: repeated-iteration-scopes
input_schema:
  type: dynamic
vars:
  items: [alpha, beta, gamma]
steps:
  - for_each: values as value
    items: "{{ vars.items }}"
    steps:
      - step: record
        handler: test::record
        args:
          index: "{{ iter.index }}"
          value: "{{ iter.value }}"
"#,
    )
    .unwrap();
    let compiled = compile_pipeline(&pipeline, &registry, CompileOptions::strict())
        .into_artifact()
        .unwrap();

    let result = Runner::new(Arc::new(HandlerRegistry::new()))
        .run_compiled(&compiled, json!(null))
        .await;

    assert_eq!(result.value().unwrap(), &json!(null));
    let recorded = invocations
        .lock()
        .unwrap()
        .iter()
        .map(|invocation| invocation.args().clone())
        .collect::<Vec<_>>();
    assert_eq!(
        recorded,
        vec![
            json!({"index": 0, "value": "alpha"}),
            json!({"index": 1, "value": "beta"}),
            json!({"index": 2, "value": "gamma"}),
        ]
    );
}
