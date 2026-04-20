use cruxx_core::prelude::CruxErr;
use cruxx_script::HandlerRegistry;
use serde_json::{Value, json};
use tokio::process::Command;

use crate::error::opt_str;

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler("git::staged_files", |input: Value| async move {
        let cwd = opt_str(&input, "cwd").map(str::to_string);
        let out = git_cmd(&["diff", "--cached", "--name-only"], cwd.as_deref()).await?;
        let files: Vec<Value> = out
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| Value::String(l.to_string()))
            .collect();
        Ok(json!({ "files": files }))
    });

    registry.handler("git::diff", |input: Value| async move {
        let cwd = opt_str(&input, "cwd").map(str::to_string);
        // Use `revision` rather than `ref` to avoid the Rust keyword at the YAML schema level.
        let git_ref = input
            .get("args")
            .and_then(|a| a.get("revision"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let args: Vec<&str> = match git_ref.as_deref() {
            Some(r) => vec!["diff", r],
            None => vec!["diff"],
        };
        let out = git_cmd(&args, cwd.as_deref()).await?;
        Ok(json!({ "diff": out }))
    });

    registry.handler("git::log", |input: Value| async move {
        let cwd = opt_str(&input, "cwd").map(str::to_string);
        let n = input
            .get("args")
            .and_then(|a| a.get("n"))
            .and_then(|v| v.as_u64())
            .unwrap_or(10);
        let n_str = format!("-{n}");
        let out = git_cmd(&["log", &n_str, "--format=%H\t%s"], cwd.as_deref()).await?;
        let commits: Vec<Value> = out
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                let mut parts = l.splitn(2, '\t');
                let hash = parts.next().unwrap_or("").to_string();
                let subject = parts.next().unwrap_or("").to_string();
                json!({ "hash": hash, "subject": subject })
            })
            .collect();
        Ok(json!({ "commits": commits }))
    });

    registry.handler("git::status", |input: Value| async move {
        let cwd = opt_str(&input, "cwd").map(str::to_string);
        let out = git_cmd(&["status", "--porcelain"], cwd.as_deref()).await?;
        let clean = out.trim().is_empty();
        Ok(json!({ "porcelain": out, "clean": clean }))
    });
}

async fn git_cmd(args: &[&str], cwd: Option<&str>) -> Result<String, CruxErr> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = cmd
        .output()
        .await
        .map_err(|e| CruxErr::step_failed("git", format!("spawn failed: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(CruxErr::step_failed(
            "git",
            format!("exit {}: {stderr}", out.status),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
