use cruxx_core::prelude::CruxErr;
use cruxx_script::HandlerRegistry;
use serde_json::{Value, json};
use tokio::process::Command;

use crate::error::{opt_str, require_str};

/// Register shell handlers: `shell::exec` (fire-and-forget) and `shell::capture` (fail on
/// non-zero exit).
///
/// Both handlers require a `cmd` arg to be present in the step's `args` field at runtime.
/// Use cruxx-script's static args injection to supply `cmd` at the pipeline-definition level;
/// without it the handler will return a `MissingArg` error.
pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value("shell::exec", |input: Value| async move {
        run_shell(input, false).await
    });

    registry.handler_value("shell::capture", |input: Value| async move {
        run_shell(input, true).await
    });
}

async fn run_shell(input: Value, fail_on_nonzero: bool) -> Result<Value, CruxErr> {
    let cmd = require_str(&input, "cmd").map_err(CruxErr::from)?;
    let cwd = opt_str(&input, "cwd");

    let mut command = Command::new("sh");
    command.arg("-c").arg(cmd);
    if let Some(dir) = cwd {
        command.current_dir(dir);
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
