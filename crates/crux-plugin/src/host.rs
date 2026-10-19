//! Plugin host: spawn, communicate with, and manage plugin subprocesses.
//!
//! Each plugin is a persistent child process. Communication is
//! newline-delimited JSON over stdin/stdout.
//!
//! ## Lifecycle
//!
//! Each loaded plugin tracks a [`PluginLifecycle`] state, readable without
//! blocking on the subprocess's I/O lock:
//!
//! - `Startup` while the process is spawning and declaring handlers.
//! - `Running` once declared and accepting invocations.
//! - `Draining` while [`PluginHost::reload_plugin`] is in progress -- any
//!   call already in flight is allowed to finish, but new calls are
//!   rejected immediately rather than queued against the outgoing process.
//! - `Failed` when the subprocess dies or its pipe breaks. [`PluginHost::invoke`]
//!   attempts a single automatic restart before giving up.
//! - `Shutdown` once [`PluginHost::shutdown_all`] has run; terminal.
//!
//! ## Sandbox
//!
//! [`PluginEntry::sandbox`] is enforced at spawn time: the child's
//! environment is cleared and only the resolved manifest `env` plus the
//! sandbox `env_allow` list are set, so a plugin can never silently inherit
//! the host's full environment. `fs_allow`, `network`, and `max_memory_mb`
//! are declared and handed to the plugin via `CRUX_SANDBOX_POLICY` (JSON)
//! for self-enforcement -- see [`crate::manifest::SandboxPolicy`] for the
//! current enforcement boundary.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock};

use crate::manifest::{PluginEntry, PluginLifecycle};
use crate::protocol::{HandlerDecl, Request, Response};

/// Maximum number of automatic restart attempts after a process failure
/// before a plugin is left in [`PluginLifecycle::Failed`] for good.
const MAX_RESTART_ATTEMPTS: u32 = 3;

/// A running plugin subprocess.
struct PluginProcess {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    /// Handler names this plugin provides.
    handlers: Vec<String>,
}

/// A loaded plugin: its subprocess, lifecycle state, and the manifest entry
/// used to (re)spawn it.
struct PluginSlot {
    process: Arc<Mutex<PluginProcess>>,
    state: Arc<RwLock<PluginLifecycle>>,
    entry: PluginEntry,
    restart_count: AtomicU32,
}

/// Manages all loaded plugins and routes handler invocations.
pub struct PluginHost {
    /// plugin name -> slot
    plugins: HashMap<String, PluginSlot>,
    /// handler name -> plugin name
    handler_map: HashMap<String, String>,
    /// All declared handlers (for introspection).
    declarations: Vec<HandlerDecl>,
}

impl PluginHost {
    /// Creates an empty host with no running plugins or handler routes.
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            handler_map: HashMap::new(),
            declarations: Vec::new(),
        }
    }

    /// Spawn a plugin, send `Declare`, register its handlers.
    ///
    /// All `env://VAR` references in the plugin entry are resolved from the
    /// host process environment before the subprocess is spawned, and the
    /// entry's [`SandboxPolicy`] is applied (see module docs).
    pub async fn load_plugin(&mut self, entry: &PluginEntry) -> Result<(), PluginError> {
        let (mut proc, handlers) = spawn_and_declare(entry).await?;
        proc.handlers = handlers.iter().map(|d| d.name.clone()).collect();

        for decl in &handlers {
            self.handler_map
                .insert(decl.name.clone(), entry.name.clone());
            self.declarations.push(decl.clone());
        }

        let slot = PluginSlot {
            process: Arc::new(Mutex::new(proc)),
            state: Arc::new(RwLock::new(PluginLifecycle::Running)),
            entry: entry.clone(),
            restart_count: AtomicU32::new(0),
        };
        self.plugins.insert(entry.name.clone(), slot);
        Ok(())
    }

    /// Current lifecycle state of a loaded plugin, or `None` if unknown.
    pub async fn lifecycle(&self, plugin: &str) -> Option<PluginLifecycle> {
        let slot = self.plugins.get(plugin)?;
        Some(*slot.state.read().await)
    }

    /// All declared handlers across all loaded plugins.
    pub fn declared_handlers(&self) -> &[HandlerDecl] {
        &self.declarations
    }

    /// Invoke a handler by name, routing to the correct plugin.
    ///
    /// Rejects with [`PluginError::NotAccepting`] if the owning plugin is
    /// not currently [`PluginLifecycle::Running`] (e.g. draining a reload,
    /// or failed). On a process-level I/O failure, attempts one automatic
    /// restart before returning an error.
    pub async fn invoke(
        &self,
        handler: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let plugin_name = self
            .handler_map
            .get(handler)
            .ok_or_else(|| PluginError::HandlerNotFound(handler.into()))?
            .clone();

        let slot = self
            .plugins
            .get(&plugin_name)
            .ok_or_else(|| PluginError::Protocol(format!("plugin '{plugin_name}' not running")))?;

        let state = *slot.state.read().await;
        if !state.accepts_invocations() {
            return Err(PluginError::NotAccepting {
                plugin: plugin_name,
                state,
            });
        }

        let req = Request::Invoke {
            handler: handler.into(),
            input,
        };

        let mut proc = slot.process.lock().await;
        match send_recv(&mut proc, &req).await {
            Ok(resp) => decode_invoke_response(handler, resp),
            Err(_io_err) => {
                // Process-level failure: try one automatic restart, then
                // retry the request exactly once.
                *slot.state.write().await = PluginLifecycle::Failed;
                restart_locked_process(&slot.entry, &mut proc, &slot.restart_count).await?;
                *slot.state.write().await = PluginLifecycle::Running;
                let resp = send_recv(&mut proc, &req).await?;
                decode_invoke_response(handler, resp)
            }
        }
    }

    /// Drain in-flight calls, then restart a plugin's subprocess in place.
    ///
    /// New calls are rejected ([`PluginError::NotAccepting`]) from the
    /// moment this is called. Already-in-flight calls (holding the process
    /// lock) are allowed to finish before the old process is torn down.
    /// Handler declarations are re-fetched and replace the plugin's prior
    /// set, so a reload may change the handlers a plugin exposes.
    pub async fn reload_plugin(&mut self, plugin: &str) -> Result<(), PluginError> {
        let slot = self
            .plugins
            .get(plugin)
            .ok_or_else(|| PluginError::Protocol(format!("plugin '{plugin}' not loaded")))?;

        *slot.state.write().await = PluginLifecycle::Draining;

        // Acquiring the lock waits for any in-flight call to finish -- this
        // is the drain.
        let mut proc = slot.process.lock().await;
        let _ = send_recv(&mut proc, &Request::Shutdown).await;
        let _ = proc.child.kill().await;

        let (new_proc, handlers) = match spawn_and_declare(&slot.entry).await {
            Ok(pair) => pair,
            Err(e) => {
                // Leave the plugin visibly failed rather than silently
                // stuck draining if the respawn itself fails.
                *slot.state.write().await = PluginLifecycle::Failed;
                return Err(e);
            }
        };
        let old_handlers = std::mem::take(&mut proc.handlers);
        *proc = new_proc;
        proc.handlers = handlers.iter().map(|d| d.name.clone()).collect();

        for decl in &handlers {
            self.handler_map
                .insert(decl.name.clone(), plugin.to_string());
        }
        self.declarations
            .retain(|d| !old_handlers.contains(&d.name));
        self.declarations.extend(handlers);

        drop(proc);
        *slot.state.write().await = PluginLifecycle::Running;
        Ok(())
    }

    /// Gracefully shut down all plugin processes.
    pub async fn shutdown_all(&mut self) {
        let names: Vec<String> = self.plugins.keys().cloned().collect();
        for name in names {
            if let Some(slot) = self.plugins.remove(&name) {
                *slot.state.write().await = PluginLifecycle::Shutdown;
                let mut proc = slot.process.lock().await;
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

fn decode_invoke_response(handler: &str, resp: Response) -> Result<serde_json::Value, PluginError> {
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

/// Attempt to respawn `entry`'s subprocess in place, replacing the contents
/// of an already-locked [`PluginProcess`]. Bumps and checks `restart_count`
/// against [`MAX_RESTART_ATTEMPTS`].
async fn restart_locked_process(
    entry: &PluginEntry,
    proc: &mut PluginProcess,
    restart_count: &AtomicU32,
) -> Result<(), PluginError> {
    let attempt = restart_count.fetch_add(1, Ordering::SeqCst) + 1;
    if attempt > MAX_RESTART_ATTEMPTS {
        return Err(PluginError::RestartExhausted {
            plugin: entry.name.clone(),
            attempts: attempt - 1,
        });
    }

    let _ = proc.child.kill().await;
    let (new_proc, handlers) = spawn_and_declare(entry).await?;
    *proc = new_proc;
    proc.handlers = handlers.iter().map(|d| d.name.clone()).collect();
    Ok(())
}

/// Spawn a sandboxed subprocess for `entry` and complete the `Declare`
/// handshake, returning the running process and its handler declarations.
async fn spawn_and_declare(
    entry: &PluginEntry,
) -> Result<(PluginProcess, Vec<HandlerDecl>), PluginError> {
    let resolved_env = entry
        .resolve_env()
        .map_err(|e| PluginError::EnvResolution(e.to_string()))?;

    let mut child = sandboxed_command(entry, &resolved_env)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| PluginError::Spawn {
            plugin: entry.name.clone(),
            source: e,
        })?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| PluginError::Protocol("stdin not available after spawn".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PluginError::Protocol("stdout not available after spawn".into()))?;
    let stdout = BufReader::new(stdout);

    let mut proc = PluginProcess {
        child,
        stdin,
        stdout,
        handlers: Vec::new(),
    };

    let resp = send_recv(&mut proc, &Request::Declare).await?;
    let handlers = match resp {
        Response::Declare { handlers } => handlers,
        other => {
            return Err(PluginError::Protocol(format!(
                "expected Declare response, got: {other:?}"
            )));
        }
    };

    Ok((proc, handlers))
}

/// Builds the `Command` for a plugin's subprocess, enforcing
/// [`SandboxPolicy`] at the environment boundary.
///
/// The child's environment is cleared (`env_clear`) so it never inherits
/// the host process's full environment -- only the manifest's resolved
/// `env` plus `sandbox.env_allow` (looked up from the host environment) are
/// set. The declarative `fs_allow`/`network`/`max_memory_mb` fields are
/// serialized into `CRUX_SANDBOX_POLICY` for the plugin to read and
/// self-enforce.
fn sandboxed_command(entry: &PluginEntry, resolved_env: &HashMap<String, String>) -> Command {
    let mut cmd = Command::new(&entry.path);
    cmd.env_clear();
    cmd.envs(resolved_env);

    for var in &entry.sandbox.env_allow {
        if let Ok(value) = std::env::var(var) {
            cmd.env(var, value);
        }
    }

    if let Ok(policy_json) = serde_json::to_string(&entry.sandbox) {
        cmd.env("CRUX_SANDBOX_POLICY", policy_json);
    }

    cmd
}

/// Send a request and read one response line.
async fn send_recv(proc: &mut PluginProcess, req: &Request) -> Result<Response, PluginError> {
    let mut line = serde_json::to_string(req).map_err(|e| PluginError::Protocol(e.to_string()))?;
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
    Spawn {
        plugin: String,
        source: std::io::Error,
    },
    #[error("plugin IO error: {0}")]
    Io(std::io::Error),
    #[error("plugin protocol error: {0}")]
    Protocol(String),
    #[error("handler not found: {0}")]
    HandlerNotFound(String),
    #[error("handler '{handler}' failed: {error}")]
    HandlerFailed { handler: String, error: String },
    #[error("handler '{handler}' declared an invalid output schema: {source}")]
    InvalidSchema {
        handler: String,
        #[source]
        source: crux_script::SchemaBuildError,
    },
    #[error("env resolution failed for plugin: {0}")]
    EnvResolution(String),
    #[error("plugin '{plugin}' is not accepting invocations (state: {state:?})")]
    NotAccepting {
        plugin: String,
        state: PluginLifecycle,
    },
    #[error("plugin '{plugin}' exhausted {attempts} restart attempt(s)")]
    RestartExhausted { plugin: String, attempts: u32 },
}
