//! crux-agentic — agentic step handlers for crux-script pipelines.
//!
//! Call `register_all(&mut registry)` to install all handlers (including stdlib
//! and optionally BAML), or call each module's `register` function individually.

pub mod adapters;
pub mod analysis;
pub mod ci;
pub mod container;
pub mod discover;
pub mod error;
pub mod handlers;
pub mod harness;
pub mod llm;
pub mod llm_step;
pub mod provider;
pub mod review;
pub mod rx;
pub mod sqlite;
pub mod triage;

pub use llm_step::LlmStep;
pub use provider::{LlmProvider, LlmRequest, LlmResponse};

use crux_runtime::prelude::CruxErr;
use crux_script::HandlerRegistry;
use serde_json::Value;
use std::future::Future;

/// Register a named agent closure so that `delegate:` pipeline steps can invoke it.
pub fn register_agent<F, Fut>(registry: &mut HandlerRegistry, name: impl Into<String>, f: F)
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value, CruxErr>> + Send + 'static,
{
    registry.agent_fn(name, f);
}

/// Register all built-in handlers into the given registry.
///
/// This includes stdlib, agentic, and (with `baml` feature) BAML handlers.
pub fn register_all(registry: &mut HandlerRegistry) {
    register_all_with_plugins(registry, Vec::new());
}

/// Register all built-in handlers, including plugin handler descriptions
/// for the planner.
pub fn register_all_with_plugins(registry: &mut HandlerRegistry, plugin_handlers: Vec<String>) {
    // Stdlib handlers
    crux_stdlib::register_all(registry);

    // Agentic handlers
    analysis::register(registry);
    ci::register(registry);
    container::register(registry);
    harness::register(registry);
    review::register(registry);
    rx::register(registry);
    sqlite::register(registry);
    triage::register(registry);
    llm::register(registry);
    llm::register_stream(registry);
    llm::register_fallback(registry);

    // BAML handlers
    #[cfg(feature = "baml")]
    crux_baml::register_all_with_plugins(registry, plugin_handlers);
    #[cfg(not(feature = "baml"))]
    let _ = plugin_handlers;
}
