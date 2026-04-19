//! Plugin host: spawn, communicate with, and manage plugin subprocesses.
//!
//! Each plugin is a persistent child process. Communication is
//! newline-delimited JSON over stdin/stdout.

use std::collections::HashMap;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use crate::manifest::PluginEntry;
use crate::protocol::{HandlerDecl, Request, Response};

/// A running plugin subprocess.
struct PluginProcess {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    /// Handler names this plugin provides.
    handlers: Vec<String>,
}

/// Manages all loaded plugins and routes handler invocations.
pub struct PluginHost {
    /// plugin name -> process
    plugins: HashMap<String, PluginProcess>,
    /// handler name -> plugin name
    handler_map: HashMap<String, String>,
    /// All declared handlers (for introspection).
    declarations: Vec<HandlerDecl>,
}

impl PluginHost {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            handler_map: HashMap::new(),
            declarations: Vec::new(),
        }
    }

    /// Spawn a plugin, send `Declare`, register its handlers.
    pub async fn load_plugin(&mut self, entry: &PluginEntry) -> Result<(), PluginError> {
        let mut child = Command::new(&entry.path)
            .envs(&entry.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .map_err(|e| PluginError::Spawn {
                plugin: entry.name.clone(),
                source: e,
            })?;

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let stdout = BufReader::new(stdout);

        let mut proc = PluginProcess {
            child,
            stdin,
            stdout,
            handlers: Vec::new(),
        };

        // Send Declare request
        let resp = send_recv(&mut proc, &Request::Declare).await?;

        let handlers = match resp {
            Response::Declare { handlers } => handlers,
            other => {
                return Err(PluginError::Protocol(format!(
                    "expected Declare response, got: {other:?}"
                )));
            }
        };

        for decl in &handlers {
            proc.handlers.push(decl.name.clone());
            self.handler_map
                .insert(decl.name.clone(), entry.name.clone());
            self.declarations.push(decl.clone());
        }

        self.plugins.insert(entry.name.clone(), proc);
        Ok(())
    }

    /// All declared handlers across all loaded plugins.
    pub fn declared_handlers(&self) -> &[HandlerDecl] {
        &self.declarations
    }

    /// Invoke a handler by name, routing to the correct plugin.
    pub async fn invoke(
        &mut self,
        handler: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let plugin_name = self
            .handler_map
            .get(handler)
            .ok_or_else(|| PluginError::HandlerNotFound(handler.into()))?
            .clone();

        let proc = self.plugins.get_mut(&plugin_name).ok_or_else(|| {
            PluginError::Protocol(format!("plugin '{plugin_name}' not running"))
        })?;

        let req = Request::Invoke {
            handler: handler.into(),
            input,
        };
        let resp = send_recv(proc, &req).await?;

        match resp {
            Response::InvokeOk { output } => Ok(output),
            Response::InvokeErr { error } => Err(PluginError::HandlerFailed {
                handler: handler.into(),
                error,
            }),
            other => Err(PluginError::Protocol(format!(
                "unexpected response: {other:?}"
            ))),
        }
    }

    /// Gracefully shut down all plugin processes.
    pub async fn shutdown_all(&mut self) {
        let names: Vec<String> = self.plugins.keys().cloned().collect();
        for name in names {
            if let Some(mut proc) = self.plugins.remove(&name) {
                let _ = send_recv(&mut proc, &Request::Shutdown).await;
                let _ = proc.child.kill().await;
            }
        }
        self.handler_map.clear();
        self.declarations.clear();
    }
}

impl Default for PluginHost {
    fn default() -> Self {
        Self::new()
    }
}

/// Send a request and read one response line.
async fn send_recv(proc: &mut PluginProcess, req: &Request) -> Result<Response, PluginError> {
    let mut line =
        serde_json::to_string(req).map_err(|e| PluginError::Protocol(e.to_string()))?;
    line.push('\n');

    proc.stdin
        .write_all(line.as_bytes())
        .await
        .map_err(PluginError::Io)?;
    proc.stdin.flush().await.map_err(PluginError::Io)?;

    let mut resp_line = String::new();
    proc.stdout
        .read_line(&mut resp_line)
        .await
        .map_err(PluginError::Io)?;

    if resp_line.trim().is_empty() {
        return Err(PluginError::Protocol(
            "plugin returned empty response".into(),
        ));
    }

    serde_json::from_str(&resp_line).map_err(|e| PluginError::Protocol(e.to_string()))
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("failed to spawn plugin '{plugin}': {source}")]
    Spawn { plugin: String, source: std::io::Error },
    #[error("plugin IO error: {0}")]
    Io(std::io::Error),
    #[error("plugin protocol error: {0}")]
    Protocol(String),
    #[error("handler not found: {0}")]
    HandlerNotFound(String),
    #[error("handler '{handler}' failed: {error}")]
    HandlerFailed { handler: String, error: String },
}
