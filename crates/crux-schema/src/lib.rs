//! Runtime-independent trace schema and inspection combinators for Crux.

pub mod crux_value;

#[cfg(any(test, feature = "test-utils"))]
pub mod testing;

pub use crux_value::{Crux, Delegation, FinalPhase, StepRecord};
