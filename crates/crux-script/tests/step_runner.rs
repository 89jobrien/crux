use std::sync::{Arc, Mutex};

use crux_script::{
    HandlerExecution, HandlerMetadata, HandlerOutput, StepFuture, StepInvocation, StepRunner,
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
            HandlerExecution::free(Ok(HandlerOutput::new(json!("recorded"))))
        })
    }
}

#[tokio::test]
async fn step_runner_receives_separate_input_and_args() {
    let invocation = Arc::new(Mutex::new(None));
    let runner = RecordingRunner {
        metadata: HandlerMetadata::new("test::record"),
        invocation: Arc::clone(&invocation),
    };

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
