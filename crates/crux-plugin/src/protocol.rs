//! JSON-RPC-like protocol for crux plugin communication.
//!
//! Messages are newline-delimited JSON on stdin/stdout.
//! Host sends `Request`, plugin replies with `Response`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Host -> Plugin request.
// TODO(automation-3): Version this protocol and add invocation IDs, streaming events,
// metered usage, deadlines, cancellation, and structured errors for agent execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    /// Ask the plugin to declare its handlers.
    Declare,
    /// Invoke a specific handler with input JSON.
    Invoke { handler: String, input: Value },
    /// Ask the plugin to shut down gracefully.
    Shutdown,
}

/// Plugin -> Host response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "data")]
pub enum Response {
    /// Handler declarations returned by `Declare`.
    Declare { handlers: Vec<HandlerDecl> },
    /// Successful handler invocation result.
    InvokeOk { output: Value },
    /// Failed handler invocation.
    InvokeErr { error: String },
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
