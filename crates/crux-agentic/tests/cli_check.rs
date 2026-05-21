/// Integration tests for `crux check` subcommand.
///
/// Tests write temporary pipeline files and invoke the check logic directly
/// via `crux_script::load_file` (same path as the binary) to avoid
/// building the binary in unit test context.
use std::io::Write;
use tempfile::NamedTempFile;

fn write_pipeline(yaml: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("temp file");
    f.write_all(yaml.as_bytes()).expect("write");
    f
}

#[test]
fn check_valid_pipeline_succeeds() {
    let yaml = r#"
pipeline: test-check
steps:
  - step: echo
    handler: shell::capture
    args:
      cmd: echo hello
"#;
    let f = write_pipeline(yaml);
    let result = crux_script::load_file(f.path().to_str().unwrap());
    assert!(result.is_ok(), "valid pipeline should parse: {:?}", result);
    let pipeline = result.unwrap();
    assert_eq!(pipeline.pipeline, "test-check");
    assert_eq!(pipeline.steps.len(), 1);
}

#[test]
fn check_invalid_pipeline_fails() {
    let yaml = "this: is: not: valid: yaml: pipeline";
    let f = write_pipeline(yaml);
    let result = crux_script::load_file(f.path().to_str().unwrap());
    assert!(result.is_err(), "invalid pipeline should fail to parse");
}

#[test]
fn check_pipeline_collects_handler_names() {
    let yaml = r#"
pipeline: handler-check
steps:
  - step: s1
    handler: shell::capture
    args:
      cmd: echo ok
  - step: s2
    handler: json::pick
    args:
      fields: [a]
"#;
    let f = write_pipeline(yaml);
    let pipeline = crux_script::load_file(f.path().to_str().unwrap()).expect("should parse");

    // Collect handler names (same logic as cmd_check in the binary).
    use crux_script::schema::StepDef;
    let handlers: Vec<String> = pipeline
        .steps
        .iter()
        .filter_map(|s| {
            if let StepDef::Step(n) = s {
                Some(n.handler.clone().unwrap_or_else(|| n.step.clone()))
            } else {
                None
            }
        })
        .collect();

    assert!(handlers.contains(&"shell::capture".to_string()));
    assert!(handlers.contains(&"json::pick".to_string()));
}

#[test]
fn check_warns_about_unregistered_handlers() {
    let yaml = r#"
pipeline: warn-check
steps:
  - step: s1
    handler: unknown::handler
"#;
    let f = write_pipeline(yaml);
    let pipeline = crux_script::load_file(f.path().to_str().unwrap()).expect("yaml parses fine");

    // Build a registry and check which handlers are missing.
    let mut reg = crux_script::HandlerRegistry::new();
    crux_agentic::register_all(&mut reg);

    use crux_script::schema::StepDef;
    let unregistered: Vec<String> = pipeline
        .steps
        .iter()
        .filter_map(|s| {
            if let StepDef::Step(n) = s {
                let name = n.handler.clone().unwrap_or_else(|| n.step.clone());
                if reg.get_handler(&name).is_none() {
                    return Some(name);
                }
            }
            None
        })
        .collect();

    assert!(
        unregistered.contains(&"unknown::handler".to_string()),
        "unknown::handler should be in unregistered list"
    );
}
