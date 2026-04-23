//! cruxx-agentic — built-in step handlers for cruxx-script pipelines.
//!
//! Call `register_all(&mut registry)` to install all handlers, or call each
//! module's `register` function individually to pick only what you need.

pub mod adapters;
pub mod handlers;
pub mod container;
pub mod sqlite;
pub mod ctrl;
pub mod error;
pub mod fs;
pub mod git;
pub mod harness;
pub mod json;
pub mod llm;
pub mod llm_step;
pub mod provider;
pub mod shell;

#[cfg(feature = "baml")]
pub mod planner;

pub use llm_step::LlmStep;
pub use provider::{LlmProvider, LlmRequest, LlmResponse};

#[cfg(feature = "baml")]
#[allow(
    clippy::derivable_impls,
    clippy::empty_line_after_doc_comments,
    clippy::map_clone,
    clippy::new_without_default,
    clippy::unwrap_or_default
)]
#[cfg(feature = "baml")]
pub(crate) mod baml_client;

use cruxx_core::prelude::CruxErr;
use cruxx_script::HandlerRegistry;
use serde_json::Value;
use std::future::Future;

/// Register a named agent closure so that `delegate:` pipeline steps can invoke it.
///
/// This is the public API for pre-registering agents without requiring a concrete
/// [`cruxx_core::prelude::Agent`] impl.  Accepts any `async Fn(Value) -> Result<Value,
/// CruxErr>`.
pub fn register_agent<F, Fut>(registry: &mut HandlerRegistry, name: impl Into<String>, f: F)
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value, CruxErr>> + Send + 'static,
{
    registry.agent_fn(name, f);
}

/// Register all built-in handlers into the given registry.
///
/// Handler names follow the pattern `module::handler`, e.g. `shell::capture`.
pub fn register_all(registry: &mut HandlerRegistry) {
    register_all_with_plugins(registry, Vec::new());
}

/// Register all built-in handlers, including plugin handler descriptions
/// for the planner.
pub fn register_all_with_plugins(registry: &mut HandlerRegistry, plugin_handlers: Vec<String>) {
    container::register(registry);
    harness::register(registry);
    shell::register(registry);
    fs::register(registry);
    git::register(registry);
    json::register(registry);
    ctrl::register(registry);
    sqlite::register(registry);
    llm::register(registry);
    llm::register_stream(registry);
    #[cfg(feature = "baml")]
    llm::register_extract(registry);
    #[cfg(feature = "baml")]
    llm::register_decompose(registry);
    #[cfg(feature = "baml")]
    planner::register_plan(registry, plugin_handlers);
    #[cfg(not(feature = "baml"))]
    let _ = plugin_handlers;
}
