/// cruxx — an agentic DSL for Rust.
///
/// Re-exports cruxx-core types and cruxx-macros proc macros.
pub use cruxx_core::*;
pub use cruxx_macros::{agent, evolve, harness};

#[cfg(feature = "script")]
pub use cruxx_script as script;
