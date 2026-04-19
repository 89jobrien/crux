//! crux-agentic — built-in step handlers for crux-script pipelines.
//!
//! Call `register_all(&mut registry)` to install all handlers, or call each
//! module's `register` function individually to pick only what you need.

pub mod adapters;
pub mod ctrl;
pub mod error;
pub mod fs;
pub mod git;
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
pub(crate) mod baml_client;

use cruxai_script::HandlerRegistry;

/// Register all built-in handlers into the given registry.
///
/// Handler names follow the pattern `module::handler`, e.g. `shell::capture`.
pub fn register_all(registry: &mut HandlerRegistry) {
    register_all_with_plugins(registry, Vec::new());
}

/// Register all built-in handlers, including plugin handler descriptions
/// for the planner.
pub fn register_all_with_plugins(registry: &mut HandlerRegistry, plugin_handlers: Vec<String>) {
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
    #[cfg(not(feature = "baml"))]
    let _ = plugin_handlers;
}
