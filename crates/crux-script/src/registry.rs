/// Handler registry — maps string names to type-erased async step handlers.
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crux_runtime::prelude::{Agent, CruxCtx};
use crux_types::error::CruxErr;
use serde_json::Value;

use crate::handler_output::{HandlerExecution, HandlerOutput};
use crate::metadata::{AgentMetadata, HandlerMetadata, SchemaBuildError};
use crate::step_runner::{StepFuture, StepInvocation, StepRunner};

/// Type-erased async handler returning outcome and invocation usage together.
///
/// [`HandlerExecution`] preserves usage on success and failure. Legacy
/// [`HandlerRegistry::handler`] registrations report unknown USD cost, explicitly
/// free registrations report zero USD, and metered registrations supply exact usage.
/// Confidence remains optional inside [`HandlerOutput`]; absent confidence is not
/// treated as a reported score.
pub type BoxHandler =
    Arc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = HandlerExecution> + Send>> + Send + Sync>;

/// Type-erased agent runner: runs a registered agent with Value input, returns Value output.
/// The function receives the input and returns a future that resolves to the output.
/// Delegation is handled by the runner creating a child CruxCtx internally.
pub type BoxAgentRunner = Arc<
    dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<Value, CruxErr>> + Send>> + Send + Sync,
>;

#[derive(Clone)]
pub(crate) struct RegisteredAgent {
    pub(crate) metadata: AgentMetadata,
    pub(crate) runner: BoxAgentRunner,
}

struct ClosureStepRunner {
    metadata: HandlerMetadata,
    handler: BoxHandler,
}

impl StepRunner for ClosureStepRunner {
    fn metadata(&self) -> &HandlerMetadata {
        &self.metadata
    }

    fn run(&self, invocation: StepInvocation) -> StepFuture<'_> {
        (self.handler)(legacy_handler_input(invocation))
    }
}

fn legacy_handler_input(invocation: StepInvocation) -> Value {
    let (input, args) = invocation.into_parts();
    match input {
        Value::Object(mut object) => {
            object.insert("args".to_string(), args);
            Value::Object(object)
        }
        input => serde_json::json!({ "input": input, "args": args }),
    }
}

/// Failure while adding an executor to the canonical registry.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// A handler name is already bound to an executor.
    #[error("step runner '{name}' is already registered")]
    DuplicateRunner { name: String },
    /// An agent name is already bound to an executor.
    #[error("agent '{name}' is already registered")]
    DuplicateAgent { name: String },
    /// A runner exposes a malformed recursive schema.
    #[error("invalid schema for '{name}': {source}")]
    InvalidSchema {
        name: String,
        source: SchemaBuildError,
    },
}

/// Registry of named handlers and agents for pipeline execution.
pub struct HandlerRegistry {
    handlers: HashMap<String, BoxHandler>,
    runners: HashMap<String, Arc<dyn StepRunner>>,
    agents: HashMap<String, RegisteredAgent>,
    metadata: HashMap<String, HandlerMetadata>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            runners: HashMap::new(),
            agents: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    /// Return the set of known handler namespaces (the prefix before `::`).
    pub fn registered_namespaces(&self) -> std::collections::HashSet<&str> {
        self.handlers
            .keys()
            .chain(self.runners.keys())
            .filter_map(|k| k.split_once("::").map(|(ns, _)| ns))
            .collect()
    }

    /// Register one contract-bearing step runner.
    pub fn register<R>(&mut self, runner: R) -> Result<(), RegistryError>
    where
        R: StepRunner + 'static,
    {
        self.register_arc(Arc::new(runner))
    }

    /// Register one shared contract-bearing step runner.
    pub fn register_arc(&mut self, runner: Arc<dyn StepRunner>) -> Result<(), RegistryError> {
        let name = runner.metadata().name.clone();
        if self.runners.contains_key(&name) || self.handlers.contains_key(&name) {
            return Err(RegistryError::DuplicateRunner { name });
        }
        Self::validate_runner_metadata(runner.metadata())?;
        self.runners.insert(name, runner);
        Ok(())
    }

    /// Look up a contract-bearing runner by name.
    pub fn runner(&self, name: &str) -> Option<Arc<dyn StepRunner>> {
        self.runners.get(name).cloned()
    }

    /// Iterate over all contract-bearing runners.
    pub fn runners(&self) -> impl Iterator<Item = &dyn StepRunner> {
        self.runners.values().map(Arc::as_ref)
    }

    fn validate_runner_metadata(metadata: &HandlerMetadata) -> Result<(), RegistryError> {
        let schemas = metadata
            .input_schema
            .iter()
            .chain(metadata.output_schema.iter())
            .chain(metadata.args.args.iter().map(|arg| &arg.schema));
        for schema in schemas {
            schema
                .validate_definition()
                .map_err(|source| RegistryError::InvalidSchema {
                    name: metadata.name.clone(),
                    source,
                })?;
        }
        Ok(())
    }

    fn register_closure(&mut self, metadata: HandlerMetadata, handler: BoxHandler) {
        let name = metadata.name.clone();
        self.metadata.insert(name.clone(), metadata.clone());
        self.handlers.insert(name.clone(), Arc::clone(&handler));
        self.runners
            .insert(name, Arc::new(ClosureStepRunner { metadata, handler }));
    }

    fn register_agent(
        &mut self,
        metadata: AgentMetadata,
        runner: BoxAgentRunner,
    ) -> Result<(), RegistryError> {
        let name = metadata.name.clone();
        if self.agents.contains_key(&name) {
            return Err(RegistryError::DuplicateAgent { name });
        }
        for schema in metadata
            .input_schema
            .iter()
            .chain(metadata.output_schema.iter())
        {
            schema
                .validate_definition()
                .map_err(|source| RegistryError::InvalidSchema {
                    name: metadata.name.clone(),
                    source,
                })?;
        }
        self.agents
            .insert(name, RegisteredAgent { metadata, runner });
        Ok(())
    }

    fn register_legacy_agent(&mut self, metadata: AgentMetadata, runner: BoxAgentRunner) {
        self.agents
            .insert(metadata.name.clone(), RegisteredAgent { metadata, runner });
    }

    /// Register handler metadata for validation and introspection.
    pub fn register_metadata(&mut self, meta: HandlerMetadata) {
        self.metadata.insert(meta.name.clone(), meta);
    }

    /// Look up metadata for a registered handler by name.
    pub fn get_metadata(&self, name: &str) -> Option<&HandlerMetadata> {
        self.metadata
            .get(name)
            .or_else(|| self.runners.get(name).map(|runner| runner.metadata()))
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
        let handler: BoxHandler = Arc::new(move |v| {
            let fut = f(v);
            Box::pin(async move { HandlerExecution::unreported(fut.await) })
        });
        self.register_closure(HandlerMetadata::new(name), handler);
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
        let handler: BoxHandler = Arc::new(move |v| {
            let fut = f(v);
            Box::pin(
                async move { HandlerExecution::unreported(fut.await.map(HandlerOutput::from)) },
            ) as Pin<Box<dyn Future<Output = HandlerExecution> + Send>>
        });
        self.register_closure(HandlerMetadata::new(name), handler);
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
        let handler: BoxHandler = Arc::new(move |v| {
            let fut = f(v);
            Box::pin(
                async move { HandlerExecution::unreported(fut.await.map(HandlerOutput::from)) },
            ) as Pin<Box<dyn Future<Output = HandlerExecution> + Send>>
        });
        self.register_closure(meta, handler);
    }

    /// Register a confidence-bearing handler that is explicitly free.
    ///
    /// Successful and failed outcomes both report explicit zero USD. Use
    /// [`handler_metered`](Self::handler_metered) for token- or cost-bearing work.
    pub fn handler_free<F, Fut>(&mut self, name: impl Into<String>, f: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<HandlerOutput, CruxErr>> + Send + 'static,
    {
        let name = name.into();
        let handler: BoxHandler = Arc::new(move |v| {
            let fut = f(v);
            Box::pin(async move { HandlerExecution::free(fut.await) })
        });
        self.register_closure(HandlerMetadata::new(name), handler);
    }

    /// Register an explicitly free confidence-bearing handler with metadata.
    ///
    /// The metadata name is registered atomically with the handler, and both
    /// successful and failed executions report explicit zero USD usage.
    pub fn handler_free_with_metadata<F, Fut>(&mut self, meta: HandlerMetadata, f: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<HandlerOutput, CruxErr>> + Send + 'static,
    {
        let handler: BoxHandler = Arc::new(move |v| {
            let fut = f(v);
            Box::pin(async move { HandlerExecution::free(fut.await) })
        });
        self.register_closure(meta, handler);
    }

    /// Register a handler that returns outcome and usage atomically.
    ///
    /// Use this for paid or token-bearing work. The returned [`HandlerExecution`]
    /// preserves measured usage even when the handler fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use crux_types::budget::{HandlerUsage, UsdAmount};
    /// use crux_types::error::CruxErr;
    /// use crux_script::{HandlerExecution, HandlerRegistry};
    ///
    /// let mut registry = HandlerRegistry::new();
    /// registry.handler_metered("provider", |_input| async {
    ///     HandlerExecution::failure(
    ///         CruxErr::step_failed("provider", "request failed"),
    ///         HandlerUsage::metered(42, UsdAmount::from_micros(125)),
    ///     )
    /// });
    /// ```
    pub fn handler_metered<F, Fut>(&mut self, name: impl Into<String>, f: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerExecution> + Send + 'static,
    {
        let name = name.into();
        let handler: BoxHandler = Arc::new(move |v| Box::pin(f(v)));
        self.register_closure(HandlerMetadata::new(name), handler);
    }

    /// Register an explicitly free plain-value handler.
    ///
    /// Values have absent confidence and failures retain explicit zero USD.
    pub fn handler_value_free<F, Fut>(&mut self, name: impl Into<String>, f: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, CruxErr>> + Send + 'static,
    {
        let name = name.into();
        let handler: BoxHandler = Arc::new(move |v| {
            let fut = f(v);
            Box::pin(async move { HandlerExecution::free(fut.await.map(HandlerOutput::from)) })
        });
        self.register_closure(HandlerMetadata::new(name), handler);
    }

    /// Register an explicitly free plain-value handler and metadata atomically.
    ///
    /// The metadata name is used as the handler name.
    pub fn handler_value_free_with_metadata<F, Fut>(&mut self, meta: HandlerMetadata, f: F)
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, CruxErr>> + Send + 'static,
    {
        let handler: BoxHandler = Arc::new(move |v| {
            let fut = f(v);
            Box::pin(async move { HandlerExecution::free(fut.await.map(HandlerOutput::from)) })
        });
        self.register_closure(meta, handler);
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
        let runner: BoxAgentRunner = Arc::new(move |input: Value| {
            let n = agent_name.clone();
            Box::pin(async move {
                let mut ctx = CruxCtx::new(&n);
                A::run(&mut ctx, input).await
            }) as Pin<Box<dyn Future<Output = Result<Value, CruxErr>> + Send>>
        });
        self.register_legacy_agent(AgentMetadata::new(name_str), runner);
    }

    /// Register a typed crux agent and reject duplicate names.
    pub fn agent_with_metadata<A>(&mut self, metadata: AgentMetadata) -> Result<(), RegistryError>
    where
        A: Agent<Input = Value, Output = Value>,
    {
        let agent_name = metadata.name.clone();
        let runner: BoxAgentRunner = Arc::new(move |input: Value| {
            let name = agent_name.clone();
            Box::pin(async move {
                let mut ctx = CruxCtx::new(&name);
                A::run(&mut ctx, input).await
            })
        });
        self.register_agent(metadata, runner)
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
        let runner: BoxAgentRunner = Arc::new(move |v| Box::pin(f(v)));
        self.register_legacy_agent(AgentMetadata::new(name), runner);
    }

    /// Register a typed async agent closure and reject duplicate names.
    pub fn agent_fn_with_metadata<F, Fut>(
        &mut self,
        metadata: AgentMetadata,
        f: F,
    ) -> Result<(), RegistryError>
    where
        F: Fn(Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, CruxErr>> + Send + 'static,
    {
        let runner: BoxAgentRunner = Arc::new(move |value| Box::pin(f(value)));
        self.register_agent(metadata, runner)
    }

    /// Look up a handler by name.
    pub fn get_handler(&self, name: &str) -> Option<&BoxHandler> {
        self.handlers.get(name)
    }

    /// Look up an agent runner by name.
    pub fn get_agent(&self, name: &str) -> Option<&BoxAgentRunner> {
        self.agents.get(name).map(|agent| &agent.runner)
    }

    /// Look up a registered agent contract by name.
    pub fn agent_metadata(&self, name: &str) -> Option<&AgentMetadata> {
        self.agents.get(name).map(|agent| &agent.metadata)
    }

    #[allow(dead_code)]
    pub(crate) fn agent_binding(&self, name: &str) -> Option<RegisteredAgent> {
        self.agents.get(name).cloned()
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crux_types::budget::{HandlerUsage, UsdAmount};
    use serde_json::json;

    #[tokio::test]
    async fn registration_variants_preserve_success_outcome_and_usage() {
        let mut registry = HandlerRegistry::new();
        registry.handler("legacy", |_v| async { Ok(HandlerOutput::new(json!(1))) });
        registry.handler_value("legacy_value", |_v| async { Ok(json!(10)) });
        registry.handler_value_with_metadata(HandlerMetadata::new("legacy_meta"), |_v| async {
            Ok(json!(11))
        });
        registry.handler_free("free", |_v| async { Ok(HandlerOutput::new(json!(2))) });
        registry.handler_metered("metered", |_v| async {
            HandlerExecution::success(
                HandlerOutput::new(json!(3)),
                HandlerUsage::metered(4, UsdAmount::from_micros(5)),
            )
        });
        registry.handler_value_free("value_free", |_v| async { Ok(json!(4)) });
        registry.handler_free_with_metadata(HandlerMetadata::new("free_meta"), |_v| async {
            Ok(HandlerOutput::with_confidence(json!(5), 0.8))
        });
        registry.handler_value_free_with_metadata(
            HandlerMetadata::new("value_free_meta"),
            |_v| async { Ok(json!(6)) },
        );

        let legacy = registry.get_handler("legacy").unwrap()(Value::Null).await;
        assert_eq!(legacy.outcome.as_ref().unwrap().value, json!(1));
        assert_eq!(legacy.usage.usd, None);
        for name in ["legacy_value", "legacy_meta"] {
            let execution = registry.get_handler(name).unwrap()(Value::Null).await;
            assert!(execution.outcome.is_ok());
            assert_eq!(execution.usage.usd, None);
        }
        let free = registry.get_handler("free").unwrap()(Value::Null).await;
        assert_eq!(free.usage.usd, Some(UsdAmount::ZERO));
        let metered = registry.get_handler("metered").unwrap()(Value::Null).await;
        assert_eq!(
            metered.usage,
            HandlerUsage::metered(4, UsdAmount::from_micros(5))
        );
        let value_free = registry.get_handler("value_free").unwrap()(Value::Null).await;
        assert_eq!(value_free.usage.usd, Some(UsdAmount::ZERO));
        let free_meta = registry.get_handler("free_meta").unwrap()(Value::Null).await;
        assert_eq!(free_meta.outcome.unwrap().confidence, Some(0.8));
        assert_eq!(free_meta.usage.usd, Some(UsdAmount::ZERO));
        assert!(registry.get_metadata("free_meta").is_some());
        let value_free_meta = registry.get_handler("value_free_meta").unwrap()(Value::Null).await;
        assert!(value_free_meta.outcome.is_ok());
        assert_eq!(value_free_meta.usage.usd, Some(UsdAmount::ZERO));
        assert!(registry.get_metadata("value_free_meta").is_some());
    }

    #[tokio::test]
    async fn registration_variants_preserve_failure_outcome_and_usage() {
        let mut registry = HandlerRegistry::new();
        registry.handler("legacy", |_v| async {
            Err(CruxErr::step_failed("legacy", "failed"))
        });
        registry.handler_value("legacy_value", |_v| async {
            Err(CruxErr::step_failed("legacy_value", "failed"))
        });
        registry.handler_value_with_metadata(HandlerMetadata::new("legacy_meta"), |_v| async {
            Err(CruxErr::step_failed("legacy_meta", "failed"))
        });
        registry.handler_free("free", |_v| async {
            Err(CruxErr::step_failed("free", "failed"))
        });
        registry.handler_metered("metered", |_v| async {
            HandlerExecution::failure(
                CruxErr::step_failed("metered", "failed"),
                HandlerUsage::metered(7, UsdAmount::from_micros(9)),
            )
        });
        registry.handler_value_free("value_free", |_v| async {
            Err(CruxErr::step_failed("value_free", "failed"))
        });
        registry.handler_free_with_metadata(HandlerMetadata::new("free_meta"), |_v| async {
            Err(CruxErr::step_failed("free_meta", "failed"))
        });
        registry.handler_value_free_with_metadata(
            HandlerMetadata::new("value_free_meta"),
            |_v| async { Err(CruxErr::step_failed("value_free_meta", "failed")) },
        );

        for name in [
            "legacy",
            "legacy_value",
            "legacy_meta",
            "free",
            "metered",
            "value_free",
            "free_meta",
            "value_free_meta",
        ] {
            let execution = registry.get_handler(name).unwrap()(Value::Null).await;
            assert!(execution.outcome.is_err(), "{name}");
            match name {
                "legacy" | "legacy_value" | "legacy_meta" => {
                    assert_eq!(execution.usage.usd, None)
                }
                "metered" => assert_eq!(
                    execution.usage,
                    HandlerUsage::metered(7, UsdAmount::from_micros(9))
                ),
                _ => assert_eq!(execution.usage.usd, Some(UsdAmount::ZERO)),
            }
        }
    }
}
