//! crux-types: Serializable wire-format types for crux.
//!
//! This crate contains only data types with serde implementations.
//! It has no runtime, no async, no LLM dependencies. Designed for
//! cross-workspace consumption (e.g., minibox trace storage).

// TODO(#97): schema/runtime split — push all combinators into a crux-schema crate
//   (no tokio, no LLM deps) so external consumers (minibox, slash, braid) can use
//   crux traces without pulling the full runtime

#[cfg(any(test, feature = "test-utils"))]
pub mod testing;

pub mod budget;
pub mod crux_value;
pub mod emission;
pub mod error;
pub mod id;
pub mod recovery;
pub mod step;
pub mod task;

#[cfg(kani)]
mod kani_proofs;
