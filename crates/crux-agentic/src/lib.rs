//! crux-agentic — built-in step handlers for crux-script pipelines.
//!
//! Call `register_all(&mut registry)` to install all handlers, or call each
//! module's `register` function individually to pick only what you need.

pub mod ctrl;
pub mod error;
pub mod fs;
pub mod git;
pub mod json;
pub mod llm;
pub mod shell;

use cruxai_script::HandlerRegistry;

/// Register all built-in handlers into the given registry.
///
/// Handler names follow the pattern `module::handler`, e.g. `shell::capture`.
pub fn register_all(registry: &mut HandlerRegistry) {
    shell::register(registry);
    fs::register(registry);
    git::register(registry);
    json::register(registry);
    ctrl::register(registry);
    llm::register(registry);
}
