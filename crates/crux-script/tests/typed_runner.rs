use std::sync::{Arc, Mutex};

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
