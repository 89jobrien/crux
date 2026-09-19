use std::sync::{Arc, Mutex};

use crux_script::{
    ConfidenceCapability, HandlerExecution, HandlerMetadata, HandlerOutput, HandlerRegistry,
    RegistryError, StepFuture, StepInvocation, StepRunner, ValueSchema,
};
use serde_json::json;

struct RecordingRunner {
    metadata: HandlerMetadata,
    invocation: Arc<Mutex<Option<StepInvocation>>>,
}

impl RecordingRunner {
    fn new(name: &str) -> Self {
        Self {
            metadata: HandlerMetadata::new(name)
                .input_schema(ValueSchema::Dynamic)
                .output_schema(ValueSchema::String)
                .confidence(ConfidenceCapability::Never),
            invocation: Arc::new(Mutex::new(None)),
        }
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
            HandlerExecution::free(Ok(HandlerOutput::new(json!("recorded"))))
        })
    }
}

#[tokio::test]
async fn step_runner_receives_separate_input_and_args() {
    let invocation = Arc::new(Mutex::new(None));
    let mut runner = RecordingRunner::new("test::record");
    runner.invocation = Arc::clone(&invocation);

    let execution = runner
        .run(StepInvocation::new(
            json!({"source": "pipeline"}),
            json!({"limit": 3}),
        ))
        .await;

    assert_eq!(execution.unwrap().value, json!("recorded"));
    let recorded = invocation.lock().unwrap();
    let recorded = recorded.as_ref().unwrap();
    assert_eq!(recorded.input(), &json!({"source": "pipeline"}));
    assert_eq!(recorded.args(), &json!({"limit": 3}));
    assert_eq!(runner.metadata().name, "test::record");
}

#[test]
fn registry_rejects_duplicate_runners() {
    let mut registry = HandlerRegistry::new();
    registry
        .register(RecordingRunner::new("test::record"))
        .unwrap();

    assert_eq!(
        registry.register(RecordingRunner::new("test::record")),
        Err(RegistryError::DuplicateRunner {
            name: "test::record".to_string(),
        })
    );
}

#[test]
fn registry_rejects_invalid_schemas() {
    let mut runner = RecordingRunner::new("test::invalid");
    runner.metadata.output_schema = Some(ValueSchema::Union {
        variants: Vec::new(),
    });
    let mut registry = HandlerRegistry::new();

    assert!(matches!(
        registry.register(runner),
        Err(RegistryError::InvalidSchema { name, .. }) if name == "test::invalid"
    ));
}

#[test]
fn registry_returns_runner_metadata() {
    let mut registry = HandlerRegistry::new();
    registry
        .register(RecordingRunner::new("test::record"))
        .unwrap();

    let runner = registry.runner("test::record").unwrap();
    assert_eq!(runner.metadata().name, "test::record");
    assert_eq!(registry.runners().count(), 1);
}
