/// Integration tests for `--strict` mode: unregistered handler detection.
///
/// Tests parse pipelines and check which handler names are missing from the
/// built-in registry, validating the core logic behind `--strict`.
use crux_script::HandlerRegistry;
use crux_script::schema::StepDef;

fn collect_handler_names(pipeline: &crux_script::schema::PipelineDef) -> Vec<String> {
    let mut names = Vec::new();
    for step in &pipeline.steps {
        match step {
            StepDef::Step(node) => {
                names.push(node.handler.clone().unwrap_or_else(|| node.step.clone()));
            }
            StepDef::Delegate(node) => {
                names.push(node.delegate.clone());
            }
            StepDef::Pipe(node) => {
                names.extend(node.stages.iter().map(|a| a.handler_name().to_string()));
            }
            StepDef::JoinAll(node) => {
                names.extend(node.arms.iter().map(|a| a.handler_name().to_string()));
            }
            StepDef::RouteOnConfidence(node) => {
                for route in &node.routes {
                    names.push(route.handler.clone());
                }
            }
            StepDef::Speculate(node) => {
                names.extend(node.arms.iter().map(|a| a.handler_name().to_string()));
            }
            StepDef::Poll(_) => {}
        }
    }
    names.sort();
    names.dedup();
    names
}

fn find_unregistered(pipeline_yaml: &str) -> Vec<String> {
    let pipeline = crux_script::load(pipeline_yaml).expect("valid pipeline YAML");
    let mut reg = HandlerRegistry::new();
    crux_agentic::register_all(&mut reg);

    let mut unregistered = Vec::new();
    for name in collect_handler_names(&pipeline) {
        if reg.get_handler(&name).is_none() && !unregistered.contains(&name) {
            unregistered.push(name);
        }
    }
    unregistered
}

#[test]
fn strict_detects_unregistered_handlers() {
    let yaml = r#"
pipeline: test-strict
steps:
  - step: do-magic
    handler: unicorn::sparkle
    args:
      glitter: true
  - step: do-more
    handler: dragon::breathe
    args:
      fire: true
"#;
    let missing = find_unregistered(yaml);
    assert!(!missing.is_empty(), "should detect unregistered handlers");
    assert!(
        missing.contains(&"unicorn::sparkle".to_string()),
        "should list unicorn::sparkle, got: {missing:?}"
    );
    assert!(
        missing.contains(&"dragon::breathe".to_string()),
        "should list dragon::breathe, got: {missing:?}"
    );
}

#[test]
fn strict_passes_registered_handlers() {
    let yaml = r#"
pipeline: test-strict-ok
steps:
  - step: run-cmd
    handler: shell::capture
    args:
      cmd: echo hello
"#;
    let missing = find_unregistered(yaml);
    assert!(
        missing.is_empty(),
        "registered handler should not be flagged: {missing:?}"
    );
}

#[test]
fn strict_collects_all_missing_at_once() {
    let yaml = r#"
pipeline: test-strict-multi
steps:
  - step: a
    handler: fake::alpha
    args: {}
  - step: b
    handler: fake::beta
    args: {}
  - step: c
    handler: fake::gamma
    args: {}
"#;
    let missing = find_unregistered(yaml);
    assert_eq!(
        missing.len(),
        3,
        "should collect all 3 unregistered handlers, got: {missing:?}"
    );
}
