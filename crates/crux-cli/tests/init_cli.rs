use std::process::Command;

#[test]
fn init_scaffolds_a_complete_crux_project() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("demo");
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .arg("init")
        .arg(&project)
        .output()
        .expect("crux init must execute");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for relative in [
        "Cargo.toml",
        "src/main.rs",
        "pipelines/main.crux",
        ".crux/policy.json",
        "tests/fixtures/input.json",
        "tests/fixtures/replay.json",
    ] {
        assert!(project.join(relative).is_file(), "missing {relative}");
    }
    let pipeline = std::fs::read_to_string(project.join("pipelines/main.crux")).unwrap();
    crux_script::load(&pipeline).unwrap();
}
