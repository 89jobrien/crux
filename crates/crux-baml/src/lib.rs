#![allow(clippy::large_enum_variant)] // generated BAML client types
//! crux-baml — BAML-powered LLM handlers for crux-script pipelines.
//!
//! Provides `llm::extract`, `llm::decompose`, and `llm::plan` handlers
//! backed by BAML structured output.

#[allow(
    clippy::derivable_impls,
    clippy::empty_line_after_doc_comments,
    clippy::map_clone,
    clippy::new_without_default,
    clippy::unwrap_or_default
)]
pub mod baml_client;

pub mod extract;
pub mod planner;

use crux_script::HandlerRegistry;

/// Register all BAML-backed handlers.
pub fn register_all(registry: &mut HandlerRegistry) {
    extract::register_extract(registry);
    extract::register_decompose(registry);
    planner::register_plan(registry, Vec::new());
}

/// Register all BAML-backed handlers with plugin handler descriptions for the planner.
pub fn register_all_with_plugins(registry: &mut HandlerRegistry, plugin_handlers: Vec<String>) {
    extract::register_extract(registry);
    extract::register_decompose(registry);
    planner::register_plan(registry, plugin_handlers);
}
