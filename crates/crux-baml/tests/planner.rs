mod mock_baml;

use crate::mock_baml::{MockBamlServer, default_responses};
use crux_baml::baml_client::async_client::B;

/// Validates that GeneratePipeline output serializes to valid YAML.
/// Uses the mock server — no API key required.
#[tokio::test]
async fn generate_pipeline_returns_valid_yaml() {
    let server = MockBamlServer::start(default_responses()).await;
    let reg = server.registry();

    let handlers = vec!["fs::read".into(), "shell::capture".into()];
    let result = B
        .GeneratePipeline
        .with_client_registry(&reg)
        .call("read a file and count lines", &handlers, None::<&str>)
        .await
        .expect("GeneratePipeline should succeed with mock");

    assert_eq!(result.pipeline, "test-pipeline");
    assert!(
        !result.steps.is_empty(),
        "pipeline should have at least one step"
    );

    // Verify YAML serialization doesn't panic (same path as generate_pipeline()).
    let yaml =
        serde_yaml::to_string(&result).expect("GeneratePipeline output should serialize to YAML");
    assert!(yaml.contains("test-pipeline"));
}
