use crux_runtime::prelude::CruxErr;
use crux_script::{
    ArgSchema, ArgType, Capability, HandlerMetadata, HandlerRegistry, RiskLevel, SideEffect,
};
use serde_json::{Value, json};
use tokio::process::Command;

use crate::error::{opt_str, require_str};

/// Register shell handlers: `shell::exec` (fire-and-forget) and `shell::capture` (fail on
/// non-zero exit).
///
/// Both handlers require a `cmd` arg to be present in the step's `args` field at runtime.
/// Use crux-script's static args injection to supply `cmd` at the pipeline-definition level;
/// without it the handler will return a `MissingArg` error.
///
/// Optional args:
/// - `cwd`: working directory for the command
/// - `env`: object of `{ "KEY": "VALUE" }` pairs injected as environment variables
pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        shell_metadata("shell::exec", false),
        |input: Value| async move { run_shell(input, false).await },
    );

    registry.handler_value_with_metadata(
        shell_metadata("shell::capture", true),
        |input: Value| async move { run_shell(input, true).await },
    );
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
                .optional("env", ArgType::Object),
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

    if fail_on_nonzero && exit_code != 0 {
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
