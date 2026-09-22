use std::process::Command;

#[test]
fn handlers_command_generates_markdown_catalog() {
    let output = Command::new(env!("CARGO_BIN_EXE_crux"))
        .args(["handlers", "--format", "markdown"])
        .output()
        .expect("crux handlers must execute");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let catalog = String::from_utf8(output.stdout).unwrap();
    assert!(catalog.contains("# Handler catalog"), "{catalog}");
    assert!(catalog.contains("shell::capture"), "{catalog}");
    assert!(catalog.contains("Risk"), "{catalog}");
    assert!(catalog.contains("Arguments"), "{catalog}");
}
