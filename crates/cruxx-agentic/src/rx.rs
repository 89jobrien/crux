use cruxx_core::prelude::CruxErr;
use cruxx_script::HandlerRegistry;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{env, path::PathBuf};
use tokio::process::Command;

use crate::error::opt_str;

/// Register rx handlers: `rx::run` (invoke a registered script by name) and
/// `rx::list` (enumerate the rx registry).
///
/// Neither handler depends on the `rx` crate — they read `registry.json` directly.
///
/// ## `rx::run`
///
/// Required args:
/// - `name`: command name as recorded in the registry
///
/// Optional args:
/// - `args`: array of strings passed to the script
/// - `registry`: override path to `registry.json`
///
/// Returns `{exit_code, stdout, stderr}`.
///
/// ## `rx::list`
///
/// Optional args:
/// - `registry`: override path to `registry.json`
///
/// Returns `{commands: [{name, runtime, source, install_path, description}]}`.
pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value("rx::run", |input: Value| async move {
        let name = input
            .get("args")
            .and_then(|a| a.get("name"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| CruxErr::step_failed("rx::run", "missing arg: name"))?
            .to_owned();

        let extra_args: Vec<String> = input
            .get("args")
            .and_then(|a| a.get("args"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let registry_path = opt_str(&input, "registry")
            .map(PathBuf::from)
            .unwrap_or_else(default_registry_path);

        let reg = load_registry(&registry_path)?;
        let entry = reg
            .commands
            .iter()
            .find(|e| e.name == name)
            .ok_or_else(|| {
                CruxErr::step_failed("rx::run", format!("command not found in registry: {name}"))
            })?;

        let mut cmd = Command::new(&entry.install_path);
        cmd.args(&extra_args);

        let output = cmd.output().await.map_err(|e| {
            CruxErr::step_failed("rx::run", format!("failed to spawn {name}: {e}"))
        })?;

        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        Ok(json!({
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        }))
    });

    registry.handler_value("rx::list", |input: Value| async move {
        let registry_path = opt_str(&input, "registry")
            .map(PathBuf::from)
            .unwrap_or_else(default_registry_path);

        let reg = load_registry(&registry_path)?;

        let commands: Vec<Value> = reg
            .commands
            .iter()
            .map(|e| {
                json!({
                    "name": e.name,
                    "runtime": e.runtime,
                    "source": e.source,
                    "install_path": e.install_path,
                    "description": e.description,
                })
            })
            .collect();

        Ok(json!({ "commands": commands }))
    });
}

// --- Registry types (mirrors rx-registry-json without depending on it) ---

#[derive(Debug, Deserialize)]
struct RegistryFile {
    commands: Vec<RegistryEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistryEntry {
    name: String,
    source: String,
    install_path: PathBuf,
    runtime: String,
    description: Option<String>,
}

fn default_registry_path() -> PathBuf {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home).join("rx").join("registry.json");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("rx")
            .join("registry.json");
    }
    PathBuf::from(".config/rx/registry.json")
}

fn load_registry(path: &PathBuf) -> Result<RegistryFile, CruxErr> {
    if !path.exists() {
        return Ok(RegistryFile {
            commands: Vec::new(),
        });
    }
    let contents = std::fs::read_to_string(path)
        .map_err(|e| CruxErr::step_failed("rx", format!("failed to read registry: {e}")))?;
    serde_json::from_str(&contents)
        .map_err(|e| CruxErr::step_failed("rx", format!("failed to parse registry: {e}")))
}
