//! Triage pipeline handlers.
//!
//! Handlers are split by domain:
//! - [`todo`] — todo parsing, urgency scoring, deduplication, grouping, gate merging
//! - [`env`] — environment probe parsing, severity classification, remediation, overhead
//! - [`worktree`] — orphaned worktree detection and branch cleanup planning
//! - [`sync`] — todo-to-issue matching, untracked identification, plan-to-commit sync
//! - [`classify`] — commit categorization, true/false positive classification, allowlist generation

use crux_script::HandlerRegistry;

mod classify;
mod env;
mod sync;
mod todo;
mod util;
mod worktree;

/// Register all triage handlers with the given registry.
pub fn register(registry: &mut HandlerRegistry) {
    todo::register(registry);
    env::register(registry);
    worktree::register(registry);
    sync::register(registry);
    classify::register(registry);
}
