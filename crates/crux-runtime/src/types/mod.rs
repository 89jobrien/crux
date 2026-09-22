//! Runtime type facade combining wire types with closure-bearing recovery types.

// Core domain types for the crux DSL.

// Re-export trace schema combinators and primitive wire-format types.
pub use crux_schema::crux_value;
pub use crux_types::budget;
pub use crux_types::error;
pub use crux_types::id;
pub use crux_types::recovery::RecoveryKind;
pub use crux_types::step;

// Keep closure-bearing Recovery<T> here (not in crux-types).
pub mod recovery;

// Keep non-wire-format types here.
pub mod evolution;
pub mod harness;
