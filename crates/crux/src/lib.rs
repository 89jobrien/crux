//! crux — an agentic DSL for Rust.
//!
//! Re-exports crux-runtime types and crux-derive proc macros.
//! This is the primary entry point for consumers of the crux workspace.

pub use crux_derive::{agent, evolve, harness};
pub use crux_runtime::*;

// TODO(#83): trace visualization / export — add a Crux<T>::to_mermaid() or similar
//   to render execution traces as diagrams for debugging and documentation

#[cfg(feature = "script")]
pub use crux_script as script;

#[cfg(test)]
mod tests {
    #[test]
    fn facade_re_exports_core_types() {
        // Verify the facade re-exports key types from crux-runtime
        let _: fn() -> crate::prelude::CruxCtx = || crate::prelude::CruxCtx::new("test");
        let _budget = crate::prelude::Budget::tokens(10);
        let _id = crate::types::id::CruxId::new();
    }
}
