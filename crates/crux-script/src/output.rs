//! Formatting for generated pipeline definitions.

use serde_json::{Value, json};

use crate::schema::{PipelineDef, StepDef};

/// Output representation for a generated pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PipelineOutputFormat {
    /// Raw pipeline YAML.
    Yaml,
    /// JSON containing the pipeline name, referenced handlers and agents, and source YAML.
    Json,
    /// Annotated YAML with a human-readable summary header.
    Pretty,
    /// A concise numbered list of referenced handlers and agents without executable YAML.
    DryRun,
    /// A handoff document containing one item per top-level step.
    Handoff,
}

/// Failure while formatting generated pipeline output.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PipelineOutputError {
    /// The generated pipeline is not valid YAML.
    #[error("invalid pipeline YAML: {0}")]
    InvalidYaml(#[from] serde_saphyr::Error),
    /// The JSON representation could not be serialized.
    #[error("failed to serialize pipeline JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Collect sorted, deduplicated handler names referenced by a pipeline.
pub fn collect_handler_names(pipeline: &PipelineDef) -> Vec<String> {
    let mut names = Vec::new();
    collect_handler_names_into(&pipeline.steps, &mut names);
    names.sort();
    names.dedup();
    names
}

/// Collect sorted, deduplicated delegated agent names referenced by a pipeline.
pub fn collect_agent_names(pipeline: &PipelineDef) -> Vec<String> {
    let mut names = Vec::new();
    collect_agent_names_into(&pipeline.steps, &mut names);
    names.sort();
    names.dedup();
    names
}

fn collect_handler_names_into(steps: &[StepDef], names: &mut Vec<String>) {
    for step in steps {
        match step {
            StepDef::Step(node) => {
                names.push(node.handler.clone().unwrap_or_else(|| node.step.clone()));
                if let Some(on_error) = &node.on_error {
                    names.push(on_error.handler.clone());
                }
            }
            StepDef::Delegate(_) => {}
            StepDef::Pipe(node) => {
                names.extend(node.stages.iter().map(|arm| arm.handler_name().to_string()));
            }
            StepDef::JoinAll(node) => {
                names.extend(node.arms.iter().map(|arm| arm.handler_name().to_string()));
            }
            StepDef::RouteOnConfidence(node) => {
                names.extend(node.routes.iter().map(|route| route.handler.clone()));
            }
            StepDef::Speculate(node) => {
                names.extend(node.arms.iter().map(|arm| arm.handler_name().to_string()));
            }
            StepDef::Poll(node) => collect_handler_names_into(&node.steps, names),
            StepDef::ForEach(node) => collect_handler_names_into(&node.steps, names),
            StepDef::While(node) => collect_handler_names_into(&node.steps, names),
            StepDef::Repeat(node) => collect_handler_names_into(&node.steps, names),
        }
    }
}

fn collect_agent_names_into(steps: &[StepDef], names: &mut Vec<String>) {
    for step in steps {
        match step {
            StepDef::Delegate(node) => names.push(node.delegate.clone()),
            StepDef::Poll(node) => collect_agent_names_into(&node.steps, names),
            StepDef::ForEach(node) => collect_agent_names_into(&node.steps, names),
            StepDef::While(node) => collect_agent_names_into(&node.steps, names),
            StepDef::Repeat(node) => collect_agent_names_into(&node.steps, names),
            StepDef::Step(_)
            | StepDef::Pipe(_)
            | StepDef::JoinAll(_)
            | StepDef::RouteOnConfidence(_)
            | StepDef::Speculate(_) => {}
        }
    }
}

/// Format generated pipeline YAML for display, serialization, or handoff.
pub fn format_pipeline_output(
    yaml: &str,
    goal: &str,
    format: PipelineOutputFormat,
) -> Result<String, PipelineOutputError> {
    if format == PipelineOutputFormat::Yaml {
        return Ok(yaml.to_string());
    }

    let pipeline: PipelineDef = crate::load(yaml)?;
    let handlers = collect_handler_names(&pipeline);
    let agents = collect_agent_names(&pipeline);

    match format {
        PipelineOutputFormat::Yaml => Ok(yaml.to_string()),
        PipelineOutputFormat::Json => {
            let steps: Vec<Value> = handlers
                .into_iter()
                .map(|handler| json!({ "handler": handler }))
                .chain(agents.into_iter().map(|agent| json!({ "agent": agent })))
                .collect();
            Ok(serde_json::to_string_pretty(&json!({
                "pipeline": pipeline.pipeline,
                "steps": steps,
                "yaml": yaml,
            }))?)
        }
        PipelineOutputFormat::Pretty => {
            let goal_comments = goal
                .lines()
                .map(|line| format!("# Goal: {line}\n"))
                .collect::<String>();
            Ok(format!(
                "# Generated pipeline: {}\n{goal_comments}# Steps: {}\n# Handlers: {}\n# Agents: {}\n#\n\n{yaml}",
                pipeline.pipeline,
                pipeline.steps.len(),
                handlers.join(", "),
                agents.join(", ")
            ))
        }
        PipelineOutputFormat::DryRun => {
            let mut output = format!(
                "Pipeline: {} ({} steps)\n\n",
                pipeline.pipeline,
                pipeline.steps.len()
            );
            for (index, name) in handlers.iter().enumerate() {
                output.push_str(&format!("  {:>2}. {name}\n", index + 1));
            }
            for (index, name) in agents.iter().enumerate() {
                output.push_str(&format!(
                    "  {:>2}. agent: {name}\n",
                    handlers.len() + index + 1
                ));
            }
            Ok(output)
        }
        PipelineOutputFormat::Handoff => format_handoff(&pipeline, goal),
    }
}

fn format_handoff(pipeline: &PipelineDef, goal: &str) -> Result<String, PipelineOutputError> {
    let project = serde_json::to_string(&pipeline.pipeline)?;
    let description = serde_json::to_string(&format!("Generated from goal: {goal}"))?;
    let mut output =
        format!("project: {project}\nid: {project}\ndescription: {description}\n\nitems:\n\n");

    for (index, step) in pipeline.steps.iter().enumerate() {
        let (id, name, handler): (&str, &str, &str) = match step {
            StepDef::Step(node) => (
                node.step.as_str(),
                node.step.as_str(),
                node.handler.as_deref().unwrap_or(&node.step),
            ),
            StepDef::Pipe(node) => (node.pipe.as_str(), node.pipe.as_str(), node.pipe.as_str()),
            StepDef::JoinAll(node) => (
                node.join_all.as_str(),
                node.join_all.as_str(),
                node.join_all.as_str(),
            ),
            StepDef::Delegate(node) => (
                node.delegate.as_str(),
                node.delegate.as_str(),
                node.delegate.as_str(),
            ),
            StepDef::Speculate(node) => (
                node.speculate.as_str(),
                node.speculate.as_str(),
                node.speculate.as_str(),
            ),
            StepDef::RouteOnConfidence(node) => (
                node.route_on_confidence.as_str(),
                node.route_on_confidence.as_str(),
                node.route_on_confidence.as_str(),
            ),
            StepDef::Poll(node) => (node.poll.as_str(), node.poll.as_str(), node.poll.as_str()),
            StepDef::ForEach(node) => (node.label(), node.label(), node.label()),
            StepDef::While(node) => (
                node.r#while.as_str(),
                node.r#while.as_str(),
                node.r#while.as_str(),
            ),
            StepDef::Repeat(node) => (
                node.repeat.as_str(),
                node.repeat.as_str(),
                node.repeat.as_str(),
            ),
        };

        output.push_str(&format!("  - id: step-{}\n", index + 1));
        let name = serde_json::to_string(name)?;
        let title = serde_json::to_string(&format!("Execute {id} via {handler}"))?;
        let description =
            serde_json::to_string(&format!("Pipeline step {}: {handler}", index + 1))?;
        output.push_str(&format!("    name: {name}\n"));
        output.push_str(&format!("    title: {title}\n"));
        output.push_str(&format!("    description: {description}\n"));
        output.push_str("    priority: P1\n");
        output.push_str("    status: open\n\n");
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PIPELINE_YAML: &str = r#"pipeline: summarize-report
steps:
  - step: fetch
    handler: fs::read
    on_error:
      handler: log::failure
  - repeat: summarize
    count: 2
    steps:
      - step: summarize-item
        handler: llm::complete
      - delegate: report-agent
"#;

    #[test]
    fn collects_nested_handlers_in_sorted_order() {
        let pipeline = crate::load(PIPELINE_YAML).expect("pipeline must parse");
        assert_eq!(
            collect_handler_names(&pipeline),
            ["fs::read", "llm::complete", "log::failure"]
        );
        assert_eq!(collect_agent_names(&pipeline), ["report-agent"]);
    }

    #[test]
    fn formats_all_pipeline_output_modes() {
        let yaml = format_pipeline_output(
            PIPELINE_YAML,
            "summarize report",
            PipelineOutputFormat::Yaml,
        )
        .expect("YAML formatting must succeed");
        assert_eq!(yaml, PIPELINE_YAML);

        let json = format_pipeline_output(
            PIPELINE_YAML,
            "summarize report",
            PipelineOutputFormat::Json,
        )
        .expect("JSON formatting must succeed");
        let value: Value = serde_json::from_str(&json).expect("output must be JSON");
        assert_eq!(value["pipeline"], "summarize-report");
        assert_eq!(value["steps"][3]["agent"], "report-agent");

        let pretty = format_pipeline_output(
            PIPELINE_YAML,
            "summarize report",
            PipelineOutputFormat::Pretty,
        )
        .expect("pretty formatting must succeed");
        assert!(pretty.starts_with("# Generated pipeline: summarize-report"));
        assert!(pretty.contains("# Agents: report-agent"));

        let dry_run = format_pipeline_output(
            PIPELINE_YAML,
            "summarize report",
            PipelineOutputFormat::DryRun,
        )
        .expect("dry-run formatting must succeed");
        assert!(dry_run.contains("1. fs::read"));
        assert!(dry_run.contains("4. agent: report-agent"));

        let handoff = format_pipeline_output(
            PIPELINE_YAML,
            "summarize report",
            PipelineOutputFormat::Handoff,
        )
        .expect("handoff formatting must succeed");
        let handoff_value: Value = serde_saphyr::from_str(&handoff).expect("handoff must be YAML");
        assert_eq!(handoff_value["project"], "summarize-report");
        assert!(handoff.contains("id: step-2"));
    }

    #[test]
    fn pretty_and_handoff_escape_multiline_goal() {
        let goal = "summarize:\nitems";
        let pretty = format_pipeline_output(PIPELINE_YAML, goal, PipelineOutputFormat::Pretty)
            .expect("pretty formatting must succeed");
        assert!(pretty.contains("# Goal: summarize:\n# Goal: items\n"));
        crate::load(
            pretty
                .split_once("\n\npipeline:")
                .map(|(_, yaml)| format!("pipeline:{yaml}"))
                .as_deref()
                .expect("pretty output must contain pipeline YAML"),
        )
        .expect("annotated YAML must remain valid");

        let handoff = format_pipeline_output(PIPELINE_YAML, goal, PipelineOutputFormat::Handoff)
            .expect("handoff formatting must succeed");
        let value: Value = serde_saphyr::from_str(&handoff).expect("handoff must be YAML");
        assert_eq!(
            value["description"],
            "Generated from goal: summarize:\nitems"
        );
    }

    #[test]
    fn structured_output_rejects_invalid_yaml() {
        let error = format_pipeline_output("not: [valid", "goal", PipelineOutputFormat::Json)
            .expect_err("invalid YAML must fail");
        assert!(matches!(error, PipelineOutputError::InvalidYaml(_)));
    }
}
