//! JSON-RPC-like protocol for crux plugin communication.
//!
//! Messages are newline-delimited JSON on stdin/stdout.
//! Host sends `Request`, plugin replies with `Response`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Current stable protocol version. Major changes are incompatible; minor changes are additive.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    pub const fn is_compatible_with(self, other: Self) -> bool {
        self.major == other.major
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InvocationId(String);

impl InvocationId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Host -> Plugin request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    /// Negotiate protocol compatibility before declaring handlers.
    Handshake { version: ProtocolVersion },
    /// Ask the plugin to declare its handlers.
    Declare,
    /// Invoke a specific handler with input JSON.
    Invoke { handler: String, input: Value },
    /// Correlated invocation with an optional host deadline.
    InvokeV1 {
        id: InvocationId,
        handler: String,
        input: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deadline_ms: Option<u64>,
    },
    /// Cancel a correlated invocation.
    Cancel { id: InvocationId },
    /// Ask the plugin to shut down gracefully.
    Shutdown,
}

/// Plugin -> Host response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "data")]
pub enum Response {
    /// Negotiated plugin protocol version and capabilities.
    Handshake {
        version: ProtocolVersion,
        #[serde(default)]
        capabilities: Vec<String>,
    },
    /// Handler declarations returned by `Declare`.
    Declare { handlers: Vec<HandlerDecl> },
    /// Successful handler invocation result.
    InvokeOk { output: Value },
    /// Failed handler invocation.
    InvokeErr { error: String },
    /// A correlated incremental invocation event.
    Event {
        id: InvocationId,
        event: StreamEvent,
    },
    /// Final correlated invocation result.
    InvokeResult {
        id: InvocationId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ProtocolError>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Value>,
    },
    /// Acknowledge shutdown.
    ShutdownAck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    Chunk { value: Value },
    Progress { completed: u64, total: Option<u64> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
}

/// A handler declared by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandlerDecl {
    /// Namespaced handler name, e.g. "github::create_issue".
    pub name: String,
    /// One-line description for planner/help output.
    pub description: String,
}
