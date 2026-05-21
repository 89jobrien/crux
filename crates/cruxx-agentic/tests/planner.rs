#[cfg(feature = "baml")]
#[tokio::test]
async fn generate_pipeline_returns_valid_yaml() {
    let yaml = crux_baml::planner::generate_pipeline("read a file and count lines", None, &[])
        .await
        .unwrap();
    // Parse as PipelineDef to validate structure
    let pipeline: cruxx_script::schema::PipelineDef =
        cruxx_script::load(&yaml).expect("generated YAML should parse as PipelineDef");
    assert!(
        !pipeline.steps.is_empty(),
        "pipeline should have at least one step"
    );
}
