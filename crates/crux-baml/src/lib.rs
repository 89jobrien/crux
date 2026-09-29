#![allow(clippy::large_enum_variant)] // generated BAML client types
//! crux-baml — BAML-powered LLM handlers for crux-script pipelines.
//!
//! Provides `llm::invoke`, `llm::invoke_with_fallback`, `llm::stream`,
//! `llm::extract`, `llm::analyze`, `llm::confidence`, `llm::decompose`, and
//! `llm::plan` handlers.
//!
//! Every LLM call a pipeline can make is routed through BAML, so provider
//! selection, retries, and output parsing live in one place and structured
//! output is available to all of them — not only the dedicated BAML handlers.

#[allow(
    clippy::derivable_impls,
    clippy::empty_line_after_doc_comments,
    clippy::map_clone,
    clippy::new_without_default,
    clippy::unwrap_or_default
)]
pub mod baml_client;

pub mod analyze;
pub mod complete;
pub mod confidence;
pub mod extract;
pub mod planner;
pub mod schema_bridge;

use crux_script::HandlerRegistry;

pub use analyze::{register as register_analyze, register_with as register_analyze_with};
pub use complete::{register, register_with};
pub use extract::{register_extract, register_extract_with};

/// Register all BAML-backed handlers.
pub fn register_all(registry: &mut HandlerRegistry) {
    register_all_with_plugins(registry, Vec::new());
}

/// Register all BAML-backed handlers with plugin handler descriptions for the planner.
pub fn register_all_with_plugins(registry: &mut HandlerRegistry, plugin_handlers: Vec<String>) {
    extract::register_extract(registry);
    extract::register_decompose(registry);
    analyze::register(registry);
    complete::register(registry);
    planner::register_plan(registry, plugin_handlers);
}

#[cfg(test)]
mod tests {
    #[test]
    fn register_all_does_not_panic() {
        let mut registry = crux_script::HandlerRegistry::new();
        super::register_all(&mut registry);
    }
}
