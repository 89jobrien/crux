/// Core domain types for the cruxx DSL.

// Re-export all wire-format types from cruxx-types.
pub use cruxx_types::budget;
pub use cruxx_types::crux_value;
pub use cruxx_types::error;
pub use cruxx_types::id;
pub use cruxx_types::recovery::RecoveryKind;
pub use cruxx_types::step;

// Keep closure-bearing Recovery<T> here (not in cruxx-types).
pub mod recovery;

// Keep non-wire-format types here.
pub mod evolution;
pub mod harness;
