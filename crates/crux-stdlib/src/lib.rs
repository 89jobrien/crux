//! crux-stdlib — standard library handlers for crux-script pipelines.
//!
//! Deterministic, non-agentic utilities: filesystem, git, JSON transforms,
//! text parsing, and control flow primitives.

pub mod ctrl;
pub mod error;
pub mod fs;
pub mod git;
pub mod json;
pub mod text;

use crux_script::HandlerRegistry;

/// Register all stdlib handlers into the given registry.
pub fn register_all(registry: &mut HandlerRegistry) {
    ctrl::register(registry);
    fs::register(registry);
    git::register(registry);
    json::register(registry);
    text::register(registry);
}
