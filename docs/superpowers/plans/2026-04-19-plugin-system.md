# cruxx Plugin System Implementation Plan

**status: done**

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable third-party integrations (GitHub, Slack, Linear,
Jira, etc.) to register as cruxx pipeline handlers via a subprocess
JSON-RPC protocol.

**Architecture:** Plugins are external binaries that speak a minimal
JSON-RPC protocol over stdin/stdout. The host discovers plugins from
a manifest file (`~/.cruxx/plugins.toml`), launches them as persistent
child processes, and proxies handler invocations through the protocol.
Built-in handlers remain compiled-in; the plugin system layers on top
of `HandlerRegistry`.

**Tech Stack:** Rust, tokio (async subprocess I/O), serde_json
(JSON-RPC messages), toml (manifest parsing)

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/cruxx-plugin/Cargo.toml` | New crate: plugin host + protocol types |
| `crates/cruxx-plugin/src/lib.rs` | Re-exports, top-level docs |
| `crates/cruxx-plugin/src/protocol.rs` | JSON-RPC message types (Declare, Invoke, Response) |
| `crates/cruxx-plugin/src/manifest.rs` | Parse `plugins.toml`, resolve plugin binaries |
| `crates/cruxx-plugin/src/host.rs` | Spawn + manage persistent plugin subprocesses |
| `crates/cruxx-plugin/src/bridge.rs` | Adaptor: register plugin handlers into `HandlerRegistry` |
| `crates/cruxx-plugin/tests/protocol.rs` | Protocol serialization round-trip tests |
| `crates/cruxx-plugin/tests/manifest.rs` | Manifest parsing tests |
| `crates/cruxx-plugin/tests/bridge.rs` | End-to-end: mock plugin binary -> registry -> invoke |
| `crates/cruxx-plugin/tests/fixtures/echo-plugin.rs` | Tiny test plugin binary (echo handler) |
| `crates/cruxx-agentic/src/bin/cruxx.rs` | Wire plugin loading into the CLI |
| `Cargo.toml` | Add cruxx-plugin to workspace deps |

---

## Task 1: Protocol types (`protocol.rs`)

**Files:**
- Create: `crates/cruxx-plugin/Cargo.toml`
- Create: `crates/cruxx-plugin/src/lib.rs`
- Create: `crates/cruxx-plugin/src/protocol.rs`
- Test: `crates/cruxx-plugin/tests/protocol.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/cruxx-plugin/tests/protocol.rs

use cruxx_plugin::protocol::{Request, Response, HandlerDecl};

#[test]
fn declare_request_round_trips() {
    let req = Request::Declare;
    let json = serde_json::to_string(&req).unwrap();
    let back: Request = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, Request::Declare));
}

#[test]
fn invoke_request_round_trips() {
    let req = Request::Invoke {
        handler: "github::create_issue".into(),
        input: serde_json::json!({"title": "test"}),
    };
    let json = serde_json::to_string(&req).unwrap();
    let back: Request = serde_json::from_str(&json).unwrap();
    match back {
        Request::Invoke { handler, input } => {
            assert_eq!(handler, "github::create_issue");
            assert_eq!(input["title"], "test");
        }
        _ => panic!("expected Invoke"),
    }
}

#[test]
fn declare_response_round_trips() {
    let resp = Response::Declare {
        handlers: vec![
            HandlerDecl {
                name: "github::create_issue".into(),
                description: "Create a GitHub issue".into(),
            },
        ],
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: Response = serde_json::from_str(&json).unwrap();
    match back {
        Response::Declare { handlers } => {
            assert_eq!(handlers.len(), 1);
            assert_eq!(handlers[0].name, "github::create_issue");
        }
        _ => panic!("expected Declare"),
    }
}

#[test]
fn invoke_ok_response_round_trips() {
    let resp = Response::InvokeOk {
        output: serde_json::json!({"id": 42}),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: Response = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, Response::InvokeOk { .. }));
}

#[test]
fn invoke_err_response_round_trips() {
    let resp = Response::InvokeErr {
        error: "not found".into(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let back: Response = serde_json::from_str(&json).unwrap();
    match back {
        Response::InvokeErr { error } => assert_eq!(error, "not found"),
        _ => panic!("expected InvokeErr"),
    }
}

#[test]
fn shutdown_request_round_trips() {
    let req = Request::Shutdown;
    let json = serde_json::to_string(&req).unwrap();
    let back: Request = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, Request::Shutdown));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p cruxx-plugin`
Expected: compilation error — `cruxx_plugin` crate doesn't exist yet.

- [ ] **Step 3: Create the crate and implement protocol types**

```toml
# crates/cruxx-plugin/Cargo.toml
[package]
name = "cruxx-plugin"
description = "Subprocess plugin host for cruxx pipelines"
version = "0.2.4"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
homepage.workspace = true
keywords.workspace = true
categories.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }
toml = "0.8"
cruxx-core = { path = "../cruxx-core", version = "0.2.4" }
cruxx-script = { path = "../cruxx-script", version = "0.2.4" }

[dev-dependencies]
tokio = { workspace = true }
tempfile = { workspace = true }
```

Add `toml = "0.8"` to `[workspace.dependencies]` in root `Cargo.toml`.

```rust
// crates/cruxx-plugin/src/lib.rs
//! cruxx-plugin -- subprocess plugin host for cruxx pipelines.
//!
//! Plugins are external binaries speaking a JSON-RPC protocol over
//! stdin/stdout. The host discovers them from a manifest, launches
//! them as persistent child processes, and bridges their handlers
//! into the cruxx `HandlerRegistry`.

pub mod protocol;
```

```rust
// crates/cruxx-plugin/src/protocol.rs
//! JSON-RPC-like protocol for cruxx plugin communication.
//!
//! Messages are newline-delimited JSON on stdin/stdout.
//! Host sends `Request`, plugin replies with `Response`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Host -> Plugin request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    /// Ask the plugin to declare its handlers.
    Declare,
    /// Invoke a specific handler with input JSON.
    Invoke {
        handler: String,
        input: Value,
    },
    /// Ask the plugin to shut down gracefully.
    Shutdown,
}

/// Plugin -> Host response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "data")]
pub enum Response {
    /// Handler declarations returned by `Declare`.
    Declare {
        handlers: Vec<HandlerDecl>,
    },
    /// Successful handler invocation result.
    InvokeOk {
        output: Value,
    },
    /// Failed handler invocation.
    InvokeErr {
        error: String,
    },
    /// Acknowledge shutdown.
    ShutdownAck,
}

/// A handler declared by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandlerDecl {
    /// Namespaced handler name, e.g. "github::create_issue".
    pub name: String,
    /// One-line description for planner/help output.
    pub description: String,
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p cruxx-plugin`
Expected: 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/cruxx-plugin/ Cargo.toml Cargo.lock
git commit -m "feat(plugin): add cruxx-plugin crate with protocol types"
```

---

## Task 2: Manifest parsing (`manifest.rs`)

**Files:**
- Create: `crates/cruxx-plugin/src/manifest.rs`
- Modify: `crates/cruxx-plugin/src/lib.rs`
- Test: `crates/cruxx-plugin/tests/manifest.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/cruxx-plugin/tests/manifest.rs

use cruxx_plugin::manifest::{PluginManifest, PluginEntry};

#[test]
fn parse_minimal_manifest() {
    let toml = r#"
[[plugin]]
name = "github"
path = "/usr/local/bin/cruxx-github"
"#;
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    assert_eq!(manifest.plugin.len(), 1);
    assert_eq!(manifest.plugin[0].name, "github");
    assert_eq!(
        manifest.plugin[0].path,
        "/usr/local/bin/cruxx-github"
    );
    assert!(manifest.plugin[0].env.is_empty());
}

#[test]
fn parse_manifest_with_env() {
    let toml = r#"
[[plugin]]
name = "slack"
path = "cruxx-slack"
env = { SLACK_TOKEN = "xoxb-test" }
"#;
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    assert_eq!(manifest.plugin[0].env["SLACK_TOKEN"], "xoxb-test");
}

#[test]
fn parse_manifest_multiple_plugins() {
    let toml = r#"
[[plugin]]
name = "github"
path = "cruxx-github"

[[plugin]]
name = "linear"
path = "cruxx-linear"
"#;
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    assert_eq!(manifest.plugin.len(), 2);
    assert_eq!(manifest.plugin[0].name, "github");
    assert_eq!(manifest.plugin[1].name, "linear");
}

#[test]
fn parse_empty_manifest() {
    let toml = "";
    let manifest: PluginManifest = toml::from_str(toml).unwrap();
    assert!(manifest.plugin.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p cruxx-plugin -- manifest`
Expected: compilation error -- `manifest` module doesn't exist.

- [ ] **Step 3: Implement manifest types**

```rust
// crates/cruxx-plugin/src/manifest.rs
//! Plugin manifest: `~/.cruxx/plugins.toml` or per-project
//! `.cruxx/plugins.toml`.
//!
//! ```toml
//! [[plugin]]
//! name = "github"
//! path = "cruxx-github"          # binary name or absolute path
//! env = { GITHUB_TOKEN = "..." } # optional env vars for the process
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Top-level manifest file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginManifest {
    #[serde(default)]
    pub plugin: Vec<PluginEntry>,
}

/// A single plugin entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    /// Plugin name, used as the handler namespace prefix.
    pub name: String,
    /// Path to the plugin binary (absolute or on PATH).
    pub path: String,
    /// Environment variables to pass to the plugin process.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Load a manifest from a TOML file path, returning `Default`
/// (empty) if the file doesn't exist.
pub fn load_manifest(
    path: impl AsRef<std::path::Path>,
) -> Result<PluginManifest, ManifestError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(PluginManifest::default());
    }
    let contents = std::fs::read_to_string(path)?;
    let manifest: PluginManifest = toml::from_str(&contents)?;
    Ok(manifest)
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("IO error reading manifest: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
}
```

Add to `lib.rs`:

```rust
pub mod manifest;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p cruxx-plugin -- manifest`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/cruxx-plugin/src/manifest.rs crates/cruxx-plugin/src/lib.rs \
  crates/cruxx-plugin/tests/manifest.rs
git commit -m "feat(plugin): manifest parsing for plugins.toml"
```

---

## Task 3: Plugin host (`host.rs`)

**Files:**
- Create: `crates/cruxx-plugin/src/host.rs`
- Modify: `crates/cruxx-plugin/src/lib.rs`
- Test: `crates/cruxx-plugin/tests/fixtures/echo-plugin.rs`
  (test binary)
- Test: `crates/cruxx-plugin/tests/host.rs`

- [ ] **Step 1: Create the echo-plugin test fixture binary**

Add to `crates/cruxx-plugin/Cargo.toml`:

```toml
[[bin]]
name = "echo-plugin"
path = "tests/fixtures/echo-plugin.rs"
required-features = ["__test-fixture"]

[features]
__test-fixture = []
```

```rust
// crates/cruxx-plugin/tests/fixtures/echo-plugin.rs
//! Minimal test plugin: declares one handler "echo::reflect" that
//! returns its input unchanged.

use std::io::{self, BufRead, Write};

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let req: serde_json::Value =
            serde_json::from_str(&line).expect("invalid JSON");

        let method = req
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let resp = match method {
            "Declare" => serde_json::json!({
                "status": "Declare",
                "data": {
                    "handlers": [
                        {
                            "name": "echo::reflect",
                            "description": "Returns input unchanged"
                        }
                    ]
                }
            }),
            "Invoke" => {
                let input = req
                    .get("params")
                    .and_then(|p| p.get("input"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                serde_json::json!({
                    "status": "InvokeOk",
                    "data": { "output": input }
                })
            }
            "Shutdown" => {
                let resp = serde_json::json!({
                    "status": "ShutdownAck"
                });
                serde_json::to_writer(&mut out, &resp).ok();
                writeln!(out).ok();
                out.flush().ok();
                break;
            }
            _ => serde_json::json!({
                "status": "InvokeErr",
                "data": { "error": format!("unknown method: {method}") }
            }),
        };

        serde_json::to_writer(&mut out, &resp).unwrap();
        writeln!(out).unwrap();
        out.flush().unwrap();
    }
}
```

- [ ] **Step 2: Write the failing host tests**

```rust
// crates/cruxx-plugin/tests/host.rs

use cruxx_plugin::host::PluginHost;
use cruxx_plugin::manifest::PluginEntry;
use std::collections::HashMap;

fn echo_entry() -> PluginEntry {
    // Assumes `cargo build` has been run; test harness builds it.
    let bin = env!("CARGO_BIN_EXE_echo-plugin");
    PluginEntry {
        name: "echo".into(),
        path: bin.into(),
        env: HashMap::new(),
    }
}

#[tokio::test]
async fn host_declares_handlers() {
    let mut host = PluginHost::new();
    host.load_plugin(&echo_entry()).await.unwrap();
    let handlers = host.declared_handlers();
    assert_eq!(handlers.len(), 1);
    assert_eq!(handlers[0].name, "echo::reflect");
}

#[tokio::test]
async fn host_invokes_handler() {
    let mut host = PluginHost::new();
    host.load_plugin(&echo_entry()).await.unwrap();
    let input = serde_json::json!({"hello": "world"});
    let output = host
        .invoke("echo::reflect", input.clone())
        .await
        .unwrap();
    assert_eq!(output, input);
}

#[tokio::test]
async fn host_invoke_unknown_handler_errors() {
    let mut host = PluginHost::new();
    host.load_plugin(&echo_entry()).await.unwrap();
    let result = host
        .invoke("echo::nonexistent", serde_json::json!({}))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn host_shutdown() {
    let mut host = PluginHost::new();
    host.load_plugin(&echo_entry()).await.unwrap();
    host.shutdown_all().await;
    // After shutdown, invoke should error
    let result = host
        .invoke("echo::reflect", serde_json::json!({}))
        .await;
    assert!(result.is_err());
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo nextest run -p cruxx-plugin --features __test-fixture
-- host`
Expected: compilation error -- `host` module doesn't exist.

- [ ] **Step 4: Implement the plugin host**

```rust
// crates/cruxx-plugin/src/host.rs
//! Plugin host: spawn, communicate with, and manage plugin
//! subprocesses.
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
    pub async fn load_plugin(
        &mut self,
        entry: &PluginEntry,
    ) -> Result<(), PluginError> {
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

        let proc = self
            .plugins
            .get_mut(&plugin_name)
            .ok_or_else(|| {
                PluginError::Protocol(format!(
                    "plugin '{plugin_name}' not running"
                ))
            })?;

        let req = Request::Invoke {
            handler: handler.into(),
            input,
        };
        let resp = send_recv(proc, &req).await?;

        match resp {
            Response::InvokeOk { output } => Ok(output),
            Response::InvokeErr { error } => {
                Err(PluginError::HandlerFailed {
                    handler: handler.into(),
                    error,
                })
            }
            other => Err(PluginError::Protocol(format!(
                "unexpected response: {other:?}"
            ))),
        }
    }

    /// Gracefully shut down all plugin processes.
    pub async fn shutdown_all(&mut self) {
        let names: Vec<String> =
            self.plugins.keys().cloned().collect();
        for name in names {
            if let Some(mut proc) = self.plugins.remove(&name) {
                let _ =
                    send_recv(&mut proc, &Request::Shutdown).await;
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
async fn send_recv(
    proc: &mut PluginProcess,
    req: &Request,
) -> Result<Response, PluginError> {
    let mut line = serde_json::to_string(req)
        .map_err(|e| PluginError::Protocol(e.to_string()))?;
    line.push('\n');

    proc.stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| PluginError::Io(e))?;
    proc.stdin
        .flush()
        .await
        .map_err(|e| PluginError::Io(e))?;

    let mut resp_line = String::new();
    proc.stdout
        .read_line(&mut resp_line)
        .await
        .map_err(|e| PluginError::Io(e))?;

    if resp_line.trim().is_empty() {
        return Err(PluginError::Protocol(
            "plugin returned empty response".into(),
        ));
    }

    serde_json::from_str(&resp_line)
        .map_err(|e| PluginError::Protocol(e.to_string()))
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
}
```

Add to `lib.rs`:

```rust
pub mod host;
```

- [ ] **Step 5: Build the echo-plugin fixture**

Run: `cargo build -p cruxx-plugin --features __test-fixture`

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo nextest run -p cruxx-plugin --features __test-fixture
-- host`
Expected: 4 tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/cruxx-plugin/src/host.rs \
  crates/cruxx-plugin/tests/host.rs \
  crates/cruxx-plugin/tests/fixtures/echo-plugin.rs \
  crates/cruxx-plugin/Cargo.toml crates/cruxx-plugin/src/lib.rs
git commit -m "feat(plugin): plugin host with subprocess management"
```

---

## Task 4: Registry bridge (`bridge.rs`)

**Files:**
- Create: `crates/cruxx-plugin/src/bridge.rs`
- Modify: `crates/cruxx-plugin/src/lib.rs`
- Test: `crates/cruxx-plugin/tests/bridge.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/cruxx-plugin/tests/bridge.rs

use std::collections::HashMap;
use std::sync::Arc;

use cruxx_plugin::bridge::register_plugins;
use cruxx_plugin::manifest::PluginEntry;
use cruxx_script::HandlerRegistry;

fn echo_entry() -> PluginEntry {
    let bin = env!("CARGO_BIN_EXE_echo-plugin");
    PluginEntry {
        name: "echo".into(),
        path: bin.into(),
        env: HashMap::new(),
    }
}

#[tokio::test]
async fn bridge_registers_plugin_handlers_in_registry() {
    let mut registry = HandlerRegistry::new();
    let entries = vec![echo_entry()];
    register_plugins(&mut registry, &entries).await.unwrap();
    assert!(
        registry.get_handler("echo::reflect").is_some(),
        "echo::reflect should be registered"
    );
}

#[tokio::test]
async fn bridge_handler_invokes_plugin() {
    let mut registry = HandlerRegistry::new();
    let entries = vec![echo_entry()];
    register_plugins(&mut registry, &entries).await.unwrap();

    let handler = registry.get_handler("echo::reflect").unwrap().clone();
    let input = serde_json::json!({"data": "test"});
    let output = handler(input.clone()).await.unwrap();
    assert_eq!(output, input);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p cruxx-plugin --features __test-fixture
-- bridge`
Expected: compilation error -- `bridge` module doesn't exist.

- [ ] **Step 3: Implement the bridge**

The bridge wraps `PluginHost` in an `Arc<Mutex<_>>` so handler
closures (which must be `Fn + Send + Sync + 'static`) can share
access to the host.

```rust
// crates/cruxx-plugin/src/bridge.rs
//! Bridge plugin handlers into `HandlerRegistry`.
//!
//! Wraps `PluginHost` in shared state so that type-erased handler
//! closures can invoke plugins at runtime.

use std::sync::Arc;

use cruxx_core::prelude::CruxErr;
use cruxx_script::HandlerRegistry;
use tokio::sync::Mutex;

use crate::host::{PluginHost, PluginError};
use crate::manifest::PluginEntry;

/// Load all plugins from the given entries and register their
/// handlers into the registry.
pub async fn register_plugins(
    registry: &mut HandlerRegistry,
    entries: &[PluginEntry],
) -> Result<(), PluginError> {
    let mut host = PluginHost::new();
    for entry in entries {
        host.load_plugin(entry).await?;
    }

    let handler_names: Vec<String> = host
        .declared_handlers()
        .iter()
        .map(|h| h.name.clone())
        .collect();

    let host = Arc::new(Mutex::new(host));

    for name in handler_names {
        let host = host.clone();
        let handler_name = name.clone();
        registry.handler(name, move |input: serde_json::Value| {
            let host = host.clone();
            let name = handler_name.clone();
            async move {
                let mut host = host.lock().await;
                host.invoke(&name, input).await.map_err(|e| {
                    CruxErr::step_failed(&name, e.to_string())
                })
            }
        });
    }

    Ok(())
}
```

Add to `lib.rs`:

```rust
pub mod bridge;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p cruxx-plugin --features __test-fixture
-- bridge`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/cruxx-plugin/src/bridge.rs \
  crates/cruxx-plugin/tests/bridge.rs \
  crates/cruxx-plugin/src/lib.rs
git commit -m "feat(plugin): bridge plugin handlers into HandlerRegistry"
```

---

## Task 5: Wire into CLI

**Files:**
- Modify: `crates/cruxx-agentic/Cargo.toml` (add cruxx-plugin dep)
- Modify: `crates/cruxx-agentic/src/bin/cruxx.rs`

- [ ] **Step 1: Add cruxx-plugin dependency**

Add to `crates/cruxx-agentic/Cargo.toml` under `[dependencies]`:

```toml
cruxx-plugin = { path = "../cruxx-plugin", version = "0.2.4" }
```

- [ ] **Step 2: Wire plugin loading into `build_registry`**

In `crates/cruxx-agentic/src/bin/cruxx.rs`, modify `build_registry`
to accept a tokio runtime handle, load the manifest, and register
plugin handlers:

```rust
// Add to imports at top:
use cruxx_plugin::manifest::load_manifest;
use cruxx_plugin::bridge::register_plugins;

// Add --plugins flag to both Run and Plan subcommands:
// In Cli::Run:
    /// Path to plugins.toml (default: ~/.cruxx/plugins.toml)
    #[arg(long)]
    plugins: Option<String>,

// In Cli::Plan:
    /// Path to plugins.toml (default: ~/.cruxx/plugins.toml)
    #[arg(long)]
    plugins: Option<String>,
```

Modify `cmd_run` to accept and use the plugins path:

```rust
fn cmd_run(
    pipeline_path: &str,
    input_path: Option<&str>,
    plugins_path: Option<&str>,
) {
    // ... existing input/pipeline loading ...

    let rt = tokio::runtime::Runtime::new().unwrap();
    let registry = rt.block_on(async {
        build_registry(&pipeline, plugins_path).await
    });
    let runner = Runner::new(Arc::new(registry));

    let start = Instant::now();
    let cruxx = rt.block_on(runner.run(&pipeline, input));
    let elapsed = start.elapsed();

    print_trace(&cruxx, elapsed);
}
```

Modify `build_registry` to be async and load plugins:

```rust
async fn build_registry(
    pipeline: &PipelineDef,
    plugins_path: Option<&str>,
) -> HandlerRegistry {
    let mut reg = HandlerRegistry::new();
    cruxx_agentic::register_all(&mut reg);

    // Load plugins from manifest
    let manifest_path = plugins_path
        .map(String::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME")
                .unwrap_or_else(|_| ".".into());
            format!("{home}/.cruxx/plugins.toml")
        });
    let manifest = load_manifest(&manifest_path)
        .unwrap_or_default();
    if !manifest.plugin.is_empty() {
        if let Err(e) =
            register_plugins(&mut reg, &manifest.plugin).await
        {
            eprintln!(
                "[cruxx] warning: failed to load plugins: {e}"
            );
        }
    }

    // Degrade unknown names to stubs (existing logic)
    for name in collect_handler_names(pipeline) {
        if reg.get_handler(&name).is_none() {
            let n = name.clone();
            reg.handler(name, move |_input: Value| {
                let handler_name = n.clone();
                async move {
                    eprintln!(
                        "[cruxx] warning: no builtin for \
                         '{handler_name}', using stub"
                    );
                    Ok(json!({
                        "_stub": handler_name,
                        "confidence": 0.5,
                        "score": 0.5,
                    }))
                }
            });
        }
    }

    reg
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build -p cruxx-agentic`
Expected: compiles. `cruxx run --help` shows `--plugins` flag.

- [ ] **Step 4: Manual smoke test**

Run: `./target/debug/cruxx run examples/extract_entities.crux`
Expected: works exactly as before (no plugins loaded, same output).

- [ ] **Step 5: Commit**

```bash
git add crates/cruxx-agentic/Cargo.toml \
  crates/cruxx-agentic/src/bin/cruxx.rs
git commit -m "feat(cli): wire plugin loading into cruxx CLI"
```

---

## Task 6: Planner awareness of plugin handlers

**Files:**
- Modify: `crates/cruxx-agentic/src/planner.rs`

- [ ] **Step 1: Extend `handler_manifest` to accept extra handlers**

The planner's `handler_manifest()` is currently a static list.
Extend `generate_pipeline` to accept additional handler
declarations from plugins so the LLM knows about them.

```rust
// In planner.rs, change the signature:
pub async fn generate_pipeline(
    goal: &str,
    constraints: Option<&str>,
    extra_handlers: &[String],
) -> Result<String, CruxErr> {
    let mut handlers = handler_manifest();
    handlers.extend_from_slice(extra_handlers);
    let result = B
        .GeneratePipeline
        .call(
            goal.to_string(),
            &handlers,
            constraints.map(str::to_string),
        )
        .await
        .map_err(|e| {
            CruxErr::step_failed("llm::plan", format!("BAML: {e}"))
        })?;
    Ok(result.yaml)
}
```

Also update `register_plan` to accept extra handlers at
registration time (pass as captured `Vec<String>`):

```rust
pub fn register_plan(
    registry: &mut HandlerRegistry,
    extra_handlers: Vec<String>,
) {
    registry.handler("llm::plan", move |input: Value| {
        let extra = extra_handlers.clone();
        async move {
            // ... existing arg extraction ...
            let mut handlers = handler_manifest();
            handlers.extend(extra);
            let result = B
                .GeneratePipeline
                .call(goal, &handlers, constraints)
                .await
                .map_err(|e| {
                    CruxErr::step_failed(
                        "llm::plan",
                        format!("BAML: {e}"),
                    )
                })?;
            Ok(json!({
                "pipeline_name": result.pipeline_name,
                "yaml": result.yaml,
            }))
        }
    });
}
```

- [ ] **Step 2: Update `register_all` signature**

In `crates/cruxx-agentic/src/lib.rs`, change `register_all` to
pass extra handlers through:

```rust
pub fn register_all(registry: &mut HandlerRegistry) {
    register_all_with_plugins(registry, Vec::new());
}

pub fn register_all_with_plugins(
    registry: &mut HandlerRegistry,
    plugin_handlers: Vec<String>,
) {
    shell::register(registry);
    fs::register(registry);
    git::register(registry);
    json::register(registry);
    ctrl::register(registry);
    llm::register(registry);
    #[cfg(feature = "baml")]
    llm::register_extract(registry);
    #[cfg(feature = "baml")]
    llm::register_decompose(registry);
    #[cfg(feature = "baml")]
    planner::register_plan(registry, plugin_handlers);
}
```

- [ ] **Step 3: Update CLI to pass plugin handler descriptions**

In `cruxx.rs`, after loading plugins, collect their descriptions
and pass to `register_all_with_plugins`:

```rust
let plugin_handler_descs: Vec<String> = manifest
    .plugin
    .iter()
    .flat_map(|_| Vec::<String>::new()) // filled by host
    .collect();
// After register_plugins succeeds, collect from host declarations
```

This requires the host to return its declarations. In
`build_registry`, replace `register_all` with
`register_all_with_plugins` passing plugin handler descriptions.

- [ ] **Step 4: Update `cmd_plan` to pass plugin handlers**

Modify `cmd_plan` to load the manifest and pass plugin handler
descriptions to `generate_pipeline`:

```rust
#[cfg(feature = "baml")]
fn cmd_plan(
    goal: &str,
    output: Option<&str>,
    constraints: Option<&str>,
    output_type: &OutputType,
    plugins_path: Option<&str>,
) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // Load plugin manifest for handler descriptions
    let manifest_path = plugins_path
        .map(String::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME")
                .unwrap_or_else(|_| ".".into());
            format!("{home}/.cruxx/plugins.toml")
        });
    let extra: Vec<String> = load_manifest(&manifest_path)
        .unwrap_or_default()
        .plugin
        .iter()
        .map(|p| {
            format!("{}::* -- plugin (see plugins.toml)", p.name)
        })
        .collect();

    let yaml = rt
        .block_on(cruxx_agentic::planner::generate_pipeline(
            goal,
            constraints,
            &extra,
        ))
        .expect("pipeline generation failed");

    // ... rest unchanged ...
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p cruxx-agentic --features baml`
Expected: compiles without errors.

- [ ] **Step 6: Run existing tests to verify no regressions**

Run: `cargo nextest run`
Expected: all 275+ tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/cruxx-agentic/src/planner.rs \
  crates/cruxx-agentic/src/lib.rs \
  crates/cruxx-agentic/src/bin/cruxx.rs
git commit -m "feat(planner): include plugin handlers in pipeline generation"
```

---

## Task 7: Documentation + final verification

**Files:**
- Create: `docs/plugins.md`

- [ ] **Step 1: Write plugin authoring docs**

```markdown
# cruxx Plugins

Plugins extend cruxx pipelines with handlers for third-party
services. A plugin is any executable that speaks the cruxx plugin
protocol over stdin/stdout.

## Quick Start

1. Create a `~/.cruxx/plugins.toml`:

   ```toml
   [[plugin]]
   name = "github"
   path = "/usr/local/bin/cruxx-github"
   env = { GITHUB_TOKEN = "ghp_..." }
   ```

2. Run a pipeline that uses plugin handlers:

   ```bash
   cruxx run my-pipeline.crux
   ```

3. Or generate a pipeline that uses plugins:

   ```bash
   cruxx plan --goal "create a GitHub issue for each TODO"
   ```

## Plugin Protocol

Plugins communicate via newline-delimited JSON on stdin/stdout.

### Declare (host -> plugin)

Request:
```json
{"method":"Declare"}
```

Response:
```json
{
  "status": "Declare",
  "data": {
    "handlers": [
      {
        "name": "github::create_issue",
        "description": "Create a GitHub issue"
      }
    ]
  }
}
```

### Invoke (host -> plugin)

Request:
```json
{
  "method": "Invoke",
  "params": {
    "handler": "github::create_issue",
    "input": {"title": "Bug report", "body": "..."}
  }
}
```

Success response:
```json
{
  "status": "InvokeOk",
  "data": {"output": {"id": 42, "url": "..."}}
}
```

Error response:
```json
{
  "status": "InvokeErr",
  "data": {"error": "authentication failed"}
}
```

### Shutdown (host -> plugin)

Request:
```json
{"method":"Shutdown"}
```

Response:
```json
{"status":"ShutdownAck"}
```

## Writing a Plugin

A plugin is any binary that:

1. Reads newline-delimited JSON from stdin
2. Writes newline-delimited JSON to stdout
3. Handles `Declare`, `Invoke`, and `Shutdown` methods

See `crates/cruxx-plugin/tests/fixtures/echo-plugin.rs` for a
minimal Rust example.

## Handler Naming

Plugin handlers use `namespace::action` format:
- `github::create_issue`
- `slack::post_message`
- `linear::create_ticket`
- `jira::transition_issue`

The namespace comes from the `name` field in `plugins.toml`.
```

- [ ] **Step 2: Full verification**

Run:
```bash
cargo check --workspace
cargo clippy --all-targets -- -D warnings
cargo nextest run
cargo nextest run -p cruxx-plugin --features __test-fixture
```

Expected: all pass, no warnings.

- [ ] **Step 3: Commit**

```bash
git add docs/plugins.md
git commit -m "docs: plugin authoring guide and protocol reference"
```

---

## Verification Checklist

1. `cargo check --workspace` -- all crates compile
2. `cargo clippy --all-targets -- -D warnings` -- no warnings
3. `cargo nextest run` -- all existing tests pass (no regressions)
4. `cargo nextest run -p cruxx-plugin --features __test-fixture`
   -- all plugin tests pass (protocol, manifest, host, bridge)
5. `./target/debug/cruxx run examples/extract_entities.crux` --
   existing pipelines still work
6. `./target/debug/cruxx run --plugins /dev/null
   examples/extract_entities.crux` -- works with empty manifest
7. Echo plugin end-to-end: register in `plugins.toml`, run a
   pipeline referencing `echo::reflect`, verify output
