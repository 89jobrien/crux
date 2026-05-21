/// Handler registry — maps string names to type-erased async step handlers.
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crux_runtime::prelude::{Agent, CruxCtx, CruxErr};
use serde_json::Value;

use crate::handler_output::HandlerOutput;
use crate::metadata::HandlerMetadata;

/// Type-erased async handler: Value in, HandlerOutput out.
///
/// Handlers that don't carry a confidence score return `HandlerOutput::new(value)`
/// (equivalent to confidence `1.0`). Use [`HandlerRegistry::handler`] for handlers
/// that emit confidence, or [`HandlerRegistry::handler_value`] for simple handlers
/// that return a plain `Value` (auto-wrapped for backward compatibility).
pub type BoxHandler = Arc<
    dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<HandlerOutput, CruxErr>> + Send>>
        + Send
        + Sync,
>;

/// Type-erased agent runner: runs a registered agent with Value input, returns Value output.
/// The function receives the input and returns a future that resolves to the output.
/// Delegation is handled by the runner creating a child CruxCtx internally.
pub type BoxAgentRunner = Arc<
    dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<Value, CruxErr>> + Send>> + Send + Sync,
>;

/// Registry of named handlers and agents for pipeline execution.
pub struct HandlerRegistry {
    handlers: HashMap<String, BoxHandler>,
    agents: HashMap<String, BoxAgentRunner>,
    metadata: HashMap<String, HandlerMetadata>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            agents: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    /// Register handler metadata for validation and introspection.
    pub fn register_metadata(&mut self, meta: HandlerMetadata) {
        self.metadata.insert(meta.name.clone(), meta);
    }

    /// Look up metadata for a registered handler by name.
    pub fn get_metadata(&self, name: &str) -> Option<&HandlerMetadata> {
        self.metadata.get(name)
    }

    /// Register a handler that returns [`HandlerOutput`] directly.
    ///
    /// Use this when the handler needs to control confidence. The returned
    /// [`HandlerOutput`] may carry `confidence: Some(f32)` or `confidence: None`.
    /// When confidence is `None`, `{{ steps.<name>.confidence }}` in templates
    /// will return an `ExprError::NoConfidence` error — it does NOT silently
    /// resolve to `1.0`. Only an explicit `Some(score)` is template-accessible.
    ///
    /// Prefer [`handler_value`](Self::handler_value) when confidence is irrelevant.
    pub fn handler<F, Fut>(&mut self, name: impl Into<String>, f: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<HandlerOutput, CruxErr>> + Send + 'static,
    {
        let name = name.into();
        self.handlers
            .insert(name, Arc::new(move |v| Box::pin(f(v))));
    }

    /// Register a handler that returns a plain [`Value`], without a confidence score.
    ///
    /// Convenience wrapper over [`handler`](Self::handler). The `Value` is auto-wrapped
    /// via `HandlerOutput::from(value)`, which sets `confidence = None`. Accessing
    /// `{{ steps.<name>.confidence }}` in templates will fail with
    /// `ExprError::NoConfidence` — handler_value steps cannot be used as input to
    /// `route_on_confidence`.
    ///
    /// Use this for handlers where confidence is not meaningful. Use
    /// [`handler`](Self::handler) when the handler must emit a real confidence score.
    pub fn handler_value<F, Fut>(&mut self, name: impl Into<String>, f: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, CruxErr>> + Send + 'static,
    {
        let name = name.into();
        self.handlers.insert(
            name,
            Arc::new(move |v| {
                let fut = f(v);
                Box::pin(async move { fut.await.map(HandlerOutput::from) })
                    as Pin<Box<dyn Future<Output = Result<HandlerOutput, CruxErr>> + Send>>
            }),
        );
    }

    /// Register a plain-value handler together with its [`HandlerMetadata`].
    ///
    /// Combines [`handler_value`](Self::handler_value) and
    /// [`register_metadata`](Self::register_metadata) in one call.  The metadata
    /// name is used as the handler name — the two must match.
    pub fn handler_value_with_metadata<F, Fut>(&mut self, meta: HandlerMetadata, f: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, CruxErr>> + Send + 'static,
    {
        let name = meta.name.clone();
        self.metadata.insert(name.clone(), meta);
        self.handlers.insert(
            name,
            Arc::new(move |v| {
                let fut = f(v);
                Box::pin(async move { fut.await.map(HandlerOutput::from) })
                    as Pin<Box<dyn Future<Output = Result<HandlerOutput, CruxErr>> + Send>>
            }),
        );
    }

    /// Register a crux Agent by name for delegation.
    ///
    /// The agent's `run` method will be called with a fresh CruxCtx.
    pub fn agent<A>(&mut self, name: impl Into<String>)
    where
        A: Agent<Input = Value, Output = Value>,
    {
        let name_str = name.into();
        let agent_name = name_str.clone();
        self.agents.insert(
            name_str,
            Arc::new(move |input: Value| {
                let n = agent_name.clone();
                Box::pin(async move {
                    let mut ctx = CruxCtx::new(&n);
                    A::run(&mut ctx, input).await
                }) as Pin<Box<dyn Future<Output = Result<Value, CruxErr>> + Send>>
            }),
        );
    }

    /// Register a named agent using a plain async closure.
    ///
    /// Unlike [`agent`](Self::agent), this does not require a concrete [`Agent`] impl —
    /// any `async Fn(Value) -> Result<Value, CruxErr>` is accepted.  This is the
    /// preferred way to register pipeline-level delegate targets programmatically.
    pub fn agent_fn<F, Fut>(&mut self, name: impl Into<String>, f: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, CruxErr>> + Send + 'static,
    {
        let name = name.into();
        self.agents.insert(name, Arc::new(move |v| Box::pin(f(v))));
    }

    /// Look up a handler by name.
    pub fn get_handler(&self, name: &str) -> Option<&BoxHandler> {
        self.handlers.get(name)
    }

    /// Look up an agent runner by name.
    pub fn get_agent(&self, name: &str) -> Option<&BoxAgentRunner> {
        self.agents.get(name)
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
