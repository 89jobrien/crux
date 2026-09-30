//! Shell command execution and capture handlers with explicit usage accounting.

/// Shell handlers: `shell::exec` (fire-and-forget) and `shell::capture`
/// (fail on non-zero exit).
///
/// Both handlers require a `cmd` arg. Optional: `cwd`, `env`, `ignore_exit`.
use std::collections::BTreeMap;
use std::process::Stdio;

use crux_runtime::prelude::CruxErr;
use crux_script::{
    ArgSchema, ArgType, Capability, HandlerMetadata, HandlerRegistry, RiskLevel, SideEffect,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::{opt_str, require_str};

/// Registers non-failing execution and fail-on-nonzero capture handlers.
pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value_free_with_metadata(
        shell_metadata("shell::exec", false),
        |input: Value| async move { run_shell(input, false).await },
    );

    registry.handler_value_free_with_metadata(
        shell_metadata("shell::capture", true),
        |input: Value| async move { run_shell(input, true).await },
    );

    registry.handler_value_free_with_metadata(
        HandlerMetadata::new("process::run")
            .describe("Run a program with a typed argument vector without shell interpolation.")
            .args(
                ArgSchema::new()
                    .required("program", ArgType::String)
                    .required("argv", ArgType::Array)
                    .optional("cwd", ArgType::String)
                    .optional("env", ArgType::Object)
                    .optional("stdin", ArgType::String)
                    .optional("fail_on_nonzero", ArgType::Boolean),
            )
            .risk(RiskLevel::High)
            .side_effects(vec![SideEffect::Process])
            .capabilities(vec![Capability::Process])
            .deterministic(false),
        run_process,
    );
}

#[derive(Debug, Deserialize)]
struct ProcessArgs {
    program: String,
    argv: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    stdin: Option<String>,
    #[serde(default = "default_true")]
    fail_on_nonzero: bool,
}

const fn default_true() -> bool {
    true
}

fn shell_metadata(name: &str, fail_on_nonzero: bool) -> HandlerMetadata {
    let description = if fail_on_nonzero {
        "Run a shell command, capture stdout/stderr, and fail on non-zero exit."
    } else {
        "Run a shell command and capture stdout/stderr without failing on non-zero exit."
    };

    HandlerMetadata::new(name)
        .describe(description)
        .args(
            ArgSchema::new()
                .required("cmd", ArgType::String)
                .optional("cwd", ArgType::String)
                .optional("env", ArgType::Object)
                .optional("ignore_exit", ArgType::Boolean),
        )
        .risk(RiskLevel::High)
        .side_effects(vec![SideEffect::Shell, SideEffect::Process])
        .capabilities(vec![Capability::Shell, Capability::Process])
        .deterministic(false)
}

async fn run_shell(input: Value, fail_on_nonzero: bool) -> Result<Value, CruxErr> {
    let cmd = require_str(&input, "cmd").map_err(CruxErr::from)?;
    let cwd = opt_str(&input, "cwd");

    let mut command = Command::new("sh");
    command.arg("-c").arg(cmd);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    if let Some(env_map) = input
        .get("args")
        .and_then(|a| a.get("env"))
        .and_then(|e| e.as_object())
    {
        for (k, v) in env_map {
            if let Some(s) = v.as_str() {
                command.env(k, s);
            }
        }
    }

    let output = command
        .output()
        .await
        .map_err(|e| CruxErr::step_failed("shell", format!("failed to spawn process: {e}")))?;

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    let ignore_exit = input
        .get("args")
        .and_then(|a| a.get("ignore_exit"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if fail_on_nonzero && !ignore_exit && exit_code != 0 {
        return Err(CruxErr::step_failed(
            "shell::capture",
            format!("command exited {exit_code}: {stderr}"),
        ));
    }

    Ok(json!({
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": stderr,
    }))
}

async fn run_process(input: Value) -> Result<Value, CruxErr> {
    let args = input
        .get("args")
        .cloned()
        .ok_or_else(|| CruxErr::step_failed("process::run", "missing args object"))?;
    let args: ProcessArgs = serde_json::from_value(args).map_err(|error| {
        CruxErr::step_failed("process::run", format!("invalid process args: {error}"))
    })?;

    let mut command = Command::new(&args.program);
    command.args(&args.argv).envs(&args.env);
    if let Some(cwd) = args.cwd {
        command.current_dir(cwd);
    }
    if args.stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|error| {
        CruxErr::step_failed(
            "process::run",
            format!("failed to spawn '{}': {error}", args.program),
        )
    })?;
    if let Some(stdin) = args.stdin
        && let Some(mut child_stdin) = child.stdin.take()
    {
        child_stdin
            .write_all(stdin.as_bytes())
            .await
            .map_err(|error| {
                CruxErr::step_failed("process::run", format!("failed to write stdin: {error}"))
            })?;
    }
    let output = child.wait_with_output().await.map_err(|error| {
        CruxErr::step_failed(
            "process::run",
            format!("failed to wait for process: {error}"),
        )
    })?;
    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if args.fail_on_nonzero && !output.status.success() {
        return Err(CruxErr::step_failed(
            "process::run",
            format!("process exited {exit_code}: {stderr}"),
        ));
    }

    Ok(json!({
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": stderr,
    }))
}
