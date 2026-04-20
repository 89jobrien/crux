/// Handler registry — maps string names to type-erased async step handlers.
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use cruxx_core::prelude::{Agent, CruxCtx, CruxErr};
use serde_json::Value;

/// Type-erased async handler: Value in, Value out.
pub type BoxHandler = Arc<
    dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<Value, CruxErr>> + Send>> + Send + Sync,
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
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            agents: HashMap::new(),
        }
    }

    /// Register a plain async handler by name.
    pub fn handler<F, Fut>(&mut self, name: impl Into<String>, f: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, CruxErr>> + Send + 'static,
    {
        let name = name.into();
        self.handlers
            .insert(name, Arc::new(move |v| Box::pin(f(v))));
    }

    /// Register a cruxx Agent by name for delegation.
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
