pub use crux_macros::{agent, evolve, harness};
/// crux — an agentic DSL for Rust.
///
/// Re-exports crux-runtime types and crux-macros proc macros.
pub use crux_runtime::*;

// TODO(#83): trace visualization / export — add a Crux<T>::to_mermaid() or similar
//   to render execution traces as diagrams for debugging and documentation

#[cfg(feature = "script")]
pub use crux_script as script;
