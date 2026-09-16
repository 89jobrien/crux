use std::process::Command;

fn run_rule_plan(goal: &str, output_type: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_crux"))
        .args([
            "plan",
            "--goal",
            goal,
            "--planner",
            "rule",
            "--output-type",
            output_type,
        ])
        .output()
        .expect("crux plan must run")
}

#[test]
fn rule_planner_honors_json_output() {
    let output = run_rule_plan("summarize the report", "json");

    assert!(
        output.status.success(),
        "crux plan failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("rule planner output must be valid JSON");
    assert_eq!(value["pipeline"], "summarize-the-report");
    assert_eq!(value["steps"][0]["handler"], "llm::complete");
}

#[test]
fn rule_planner_formats_unsafe_goal_in_every_output_mode() {
    let goal = "summarize report:\nnext";
    let yaml = run_rule_plan(goal, "yaml");
    assert!(yaml.status.success());
    crux_script::load(&String::from_utf8_lossy(&yaml.stdout)).expect("YAML output must parse");

    let json = run_rule_plan(goal, "json");
    assert!(json.status.success());
    serde_json::from_slice::<serde_json::Value>(&json.stdout).expect("JSON output must parse");

    let pretty = run_rule_plan(goal, "pretty");
    assert!(pretty.status.success());
    let pretty = String::from_utf8(pretty.stdout).expect("pretty output must be UTF-8");
    let (_, pipeline_yaml) = pretty
        .split_once("\n\npipeline:")
        .expect("pretty output must contain pipeline YAML");
    crux_script::load(&format!("pipeline:{pipeline_yaml}"))
        .expect("pretty pipeline YAML must parse");

    let dry_run = run_rule_plan(goal, "dry-run");
    assert!(dry_run.status.success());
    assert!(String::from_utf8_lossy(&dry_run.stdout).contains("Pipeline: summarize-report-next"));

    let handoff = run_rule_plan(goal, "handoff");
    assert!(handoff.status.success());
    let handoff = String::from_utf8_lossy(&handoff.stdout);
    assert!(handoff.contains("project: \"summarize-report-next\""));
    assert!(handoff.contains("Generated from goal: summarize report:\\nnext"));

    let punctuation_only = run_rule_plan("-", "json");
    assert!(punctuation_only.status.success());
    let value: serde_json::Value =
        serde_json::from_slice(&punctuation_only.stdout).expect("fallback name must be valid JSON");
    assert_eq!(value["pipeline"], "generated-pipeline");
}

#[test]
fn rule_planner_quotes_yaml_scalar_pipeline_names() {
    for goal in ["true", "null", "123"] {
        let output = run_rule_plan(goal, "json");
        assert!(output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .expect("scalar-looking pipeline name must remain a JSON string");
        assert_eq!(value["pipeline"], goal);
    }
}

#[test]
fn rule_planner_writes_selected_format_to_file() {
    let file = tempfile::NamedTempFile::new().expect("temporary output file must be created");
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args([
            "plan",
            "--goal",
            "summarize report",
            "--planner",
            "rule",
            "--output-type",
            "json",
            "--output",
            file.path().to_str().expect("temporary path must be UTF-8"),
        ])
        .output()
        .expect("crux plan must run");
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_reader(file).expect("file must contain JSON");
    assert_eq!(value["pipeline"], "summarize-report");
}

#[test]
fn rule_planner_reports_output_write_failure() {
    let directory = tempfile::tempdir().expect("temporary directory must be created");
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args([
            "plan",
            "--goal",
            "summarize report",
            "--planner",
            "rule",
            "--output",
            directory
                .path()
                .to_str()
                .expect("temporary path must be UTF-8"),
        ])
        .output()
        .expect("crux plan must run");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to write generated pipeline"));
}
