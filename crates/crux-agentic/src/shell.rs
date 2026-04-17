use cruxai_core::prelude::CruxErr;
use cruxai_script::HandlerRegistry;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::error::{opt_str, require_str};

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler("shell::exec", |input: Value| async move {
        run_shell(input, false).await
    });

    registry.handler("shell::capture", |input: Value| async move {
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

    let output = command.output().await.map_err(|e| {
        CruxErr::step_failed("shell", format!("failed to spawn process: {e}"))
    })?;

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
