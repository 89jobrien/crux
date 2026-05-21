//! crux-types: Serializable wire-format types for crux.
//!
//! This crate contains only data types with serde implementations.
//! It has no runtime, no async, no LLM dependencies. Designed for
//! cross-workspace consumption (e.g., minibox trace storage).

#[cfg(any(test, feature = "test-utils"))]
pub mod testing;

pub mod budget;
pub mod crux_value;
pub mod error;
pub mod id;
pub mod recovery;
pub mod step;
