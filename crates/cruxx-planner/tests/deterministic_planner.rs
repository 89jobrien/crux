use cruxx_planner::deterministic::{DeterministicPlanner, PlannerConfig};

// ── helpers ─────────────────────────────────────────────────────────────────

fn planner() -> DeterministicPlanner {
    DeterministicPlanner::new(PlannerConfig::default())
}

// ── Issue #22 tests ──────────────────────────────────────────────────────────

#[test]
fn plan_read_file_goal_contains_fs_read() {
    let yaml = planner()
        .plan("Read a file and print its contents")
        .unwrap();
    assert!(yaml.contains("fs::read"), "expected fs::read in:\n{yaml}");
    assert!(
        yaml.contains("pipeline:"),
        "expected 'pipeline:' key in:\n{yaml}"
    );
    assert!(yaml.contains("steps:"), "expected 'steps:' in:\n{yaml}");
}

#[test]
fn plan_git_diff_goal_contains_git_handler() {
    let yaml = planner()
        .plan("Review a git commit and summarize changes")
        .unwrap();
    assert!(yaml.contains("git::diff"), "expected git::diff in:\n{yaml}");
}

#[test]
fn plan_extract_entities_goal_contains_llm_extract() {
    let yaml = planner().plan("Extract named entities from text").unwrap();
    assert!(
        yaml.contains("llm::extract"),
        "expected llm::extract in:\n{yaml}"
    );
}

#[test]
fn plan_write_json_goal_contains_json_write() {
    let yaml = planner()
        .plan("Write extracted data to a JSON file")
        .unwrap();
    assert!(
        yaml.contains("json::write"),
        "expected json::write in:\n{yaml}"
    );
}

#[test]
fn plan_unknown_goal_returns_shell_capture_fallback() {
    let yaml = planner().plan("xyzzy frobnicate").unwrap();
    assert!(
        yaml.contains("shell::capture"),
        "expected shell::capture fallback in:\n{yaml}"
    );
}

#[test]
fn plan_is_deterministic_same_output_twice() {
    let goal = "Read a file and extract entities";
    let first = planner().plan(goal).unwrap();
    let second = planner().plan(goal).unwrap();
    assert_eq!(first, second, "plan() must be deterministic");
}

#[test]
fn plan_output_is_valid_yaml() {
    let yaml = planner().plan("Summarize a document").unwrap();
    // Minimal validity: must have pipeline: and steps: on separate lines
    let lines: Vec<&str> = yaml.lines().collect();
    let has_pipeline = lines.iter().any(|l| l.starts_with("pipeline:"));
    let has_steps = lines.iter().any(|l| l.trim_start().starts_with("steps:"));
    assert!(has_pipeline, "missing pipeline: key");
    assert!(has_steps, "missing steps: key");
}
