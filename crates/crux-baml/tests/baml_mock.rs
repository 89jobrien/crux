//! BAML function tests using MockLLM — no API keys required.
//!
//! Each test starts a local mock OpenAI server, creates a `ClientRegistry`
//! pointing at it, and calls the BAML function via `B.Function.with_client_registry`.

mod mock_baml;

use crate::mock_baml::{MockBamlServer, default_responses};
use crux_baml::baml_client::async_client::B;

#[tokio::test]
async fn mock_extract_entities() {
    let server = MockBamlServer::start(default_responses()).await;
    let reg = server.registry();

    let result = B
        .ExtractEntities
        .with_client_registry(&reg)
        .call("Rust was created by Mozilla")
        .await
        .expect("ExtractEntities should succeed with mock");

    assert_eq!(result.entities.len(), 2);
    assert_eq!(result.entities[0].name, "Rust");
    assert_eq!(result.entities[0].entity_type, "CONCEPT");
    assert_eq!(result.entities[1].name, "Mozilla");
}

#[tokio::test]
async fn mock_summarize() {
    let server = MockBamlServer::start(default_responses()).await;
    let reg = server.registry();

    let result = B
        .Summarize
        .with_client_registry(&reg)
        .call("Some long text to summarize", 2)
        .await
        .expect("Summarize should succeed with mock");

    assert!(!result.summary.is_empty());
    assert_eq!(result.key_points.len(), 2);
    assert_eq!(result.word_count, 42);
}

#[tokio::test]
async fn mock_classify() {
    let server = MockBamlServer::start(default_responses()).await;
    let reg = server.registry();

    let labels = vec!["positive".into(), "negative".into(), "neutral".into()];
    let result = B
        .Classify
        .with_client_registry(&reg)
        .call("I love this!", &labels)
        .await
        .expect("Classify should succeed with mock");

    assert_eq!(result.label, "positive");
    assert!((0.0..=1.0).contains(&result.confidence));
}

#[tokio::test]
async fn mock_classify_ci_failure() {
    let server = MockBamlServer::start(default_responses()).await;
    let reg = server.registry();

    let patterns = vec!["test fixture values".into()];
    let result = B
        .ClassifyCIFailure
        .with_client_registry(&reg)
        .call("error: obfsck detected api_key in test.rs", &patterns)
        .await
        .expect("ClassifyCIFailure should succeed with mock");

    assert_eq!(result.kind, "false-positive");
    assert_eq!(result.fix_type, "obfsck-ignore");
    assert!(result.confidence > 0.5);
}

#[tokio::test]
async fn mock_describe_project() {
    let server = MockBamlServer::start(default_responses()).await;
    let reg = server.registry();

    let commits = vec!["feat: add mock".into()];
    let result = B
        .DescribeProject
        .with_client_registry(&reg)
        .call("crux", Some("Rust"), Some("Agentic DSL"), &commits)
        .await
        .expect("DescribeProject should succeed with mock");

    assert!(!result.description.is_empty());
}

#[tokio::test]
async fn mock_assess_health() {
    let server = MockBamlServer::start(default_responses()).await;
    let reg = server.registry();

    let dates = vec!["2026-05-20".into()];
    let result = B
        .AssessHealth
        .with_client_registry(&reg)
        .call("crux", "2026-05-20", &dates, Some(3))
        .await
        .expect("AssessHealth should succeed with mock");

    assert_eq!(result.status, "active");
    assert!(result.confidence > 0.5);
}

#[tokio::test]
async fn mock_classify_project() {
    let server = MockBamlServer::start(default_responses()).await;
    let reg = server.registry();

    let topics = vec!["rust".into(), "dsl".into()];
    let commits = vec!["feat: add mock".into()];
    let result = B
        .ClassifyProject
        .with_client_registry(&reg)
        .call("crux", Some("Agentic DSL"), Some("Rust"), &topics, &commits)
        .await
        .expect("ClassifyProject should succeed with mock");

    assert_eq!(result.category, "library");
}

#[tokio::test]
async fn mock_generate_changelog() {
    let server = MockBamlServer::start(default_responses()).await;
    let reg = server.registry();

    let commits = vec!["feat: mock tests".into(), "fix: lint".into()];
    let result = B
        .GenerateChangelog
        .with_client_registry(&reg)
        .call("crux", &commits)
        .await
        .expect("GenerateChangelog should succeed with mock");

    assert!(!result.summary.is_empty());
    assert!(!result.highlights.is_empty());
}

#[tokio::test]
async fn mock_suggest_related() {
    let server = MockBamlServer::start(default_responses()).await;
    let reg = server.registry();

    let all = vec![
        "minibox".into(),
        "devkit".into(),
        "braid".into(),
        "doob".into(),
    ];
    let result = B
        .SuggestRelated
        .with_client_registry(&reg)
        .call("crux", Some("Agentic DSL"), Some("library"), &all)
        .await
        .expect("SuggestRelated should succeed with mock");

    assert_eq!(result.related.len(), 3);
}

#[tokio::test]
async fn mock_decompose_spec() {
    let server = MockBamlServer::start(default_responses()).await;
    let reg = server.registry();

    let result = B
        .DecomposeSpec
        .with_client_registry(&reg)
        .call("Build a mock BAML server for testing")
        .await
        .expect("DecomposeSpec should succeed with mock");

    assert!(!result.tasks.is_empty());
    assert_eq!(result.tasks[0].id, "add-mock");
}

#[tokio::test]
async fn mock_generate_pipeline() {
    let server = MockBamlServer::start(default_responses()).await;
    let reg = server.registry();

    let handlers = vec!["fs::read".into(), "llm::extract".into()];
    let result = B
        .GeneratePipeline
        .with_client_registry(&reg)
        .call("Read a file and summarize it", &handlers, None::<&str>)
        .await
        .expect("GeneratePipeline should succeed with mock");

    assert_eq!(result.pipeline, "test-pipeline");
    assert!(!result.steps.is_empty());
}
