use std::path::Path;

pub fn cmd_init(path: &str) {
    if let Err(error) = scaffold(Path::new(path)) {
        eprintln!("failed to initialize {path}: {error}");
        std::process::exit(1);
    }
    println!("initialized Crux project at {path}");
}

fn scaffold(root: &Path) -> std::io::Result<()> {
    if root.exists() && root.read_dir()?.next().is_some() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "destination is not empty",
        ));
    }
    for directory in ["src", "pipelines", ".crux", "tests/fixtures"] {
        std::fs::create_dir_all(root.join(directory))?;
    }
    write(
        root,
        "Cargo.toml",
        r#"[package]
name = "crux-app"
version = "0.1.0"
edition = "2024"

[dependencies]
crux-script = "0.4"
serde_json = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
"#,
    )?;
    write(
        root,
        "src/main.rs",
        r#"use crux_script::{HandlerRegistry, Runner, load_file};
use serde_json::{Value, json};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let mut registry = HandlerRegistry::new();
    registry.handler_value("app::hello", |input: Value| async move {
        Ok(json!({ "message": "hello from Crux", "input": input }))
    });
    let pipeline = load_file("pipelines/main.crux").expect("valid pipeline");
    let trace = Runner::new(Arc::new(registry)).run(&pipeline, Value::Null).await;
    println!("{}", serde_json::to_string_pretty(&trace.value).unwrap());
}
"#,
    )?;
    write(
        root,
        "pipelines/main.crux",
        "pipeline: hello\nsteps:\n  - step: greet\n    handler: app::hello\n",
    )?;
    write(
        root,
        ".crux/policy.json",
        "{\n  \"allowed_capabilities\": [],\n  \"require_approval_above\": \"medium\"\n}\n",
    )?;
    write(root, "tests/fixtures/input.json", "{}\n")?;
    write(
        root,
        "tests/fixtures/replay.json",
        "{\n  \"agent\": \"hello\",\n  \"steps\": [],\n  \"value\": null\n}\n",
    )
}

fn write(root: &Path, relative: &str, contents: &str) -> std::io::Result<()> {
    std::fs::write(root.join(relative), contents)
}
