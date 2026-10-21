//! crux-types: Serializable wire-format types for crux.
//!
//! This crate contains only data types with serde implementations.
//! It has no runtime, no async, no LLM dependencies. Designed for
//! cross-workspace consumption (e.g., minibox trace storage).

#[cfg(any(test, feature = "test-utils"))]
pub mod testing;

pub mod approval;
pub mod budget;
pub mod emission;
pub mod error;
pub mod evolution;
pub mod execution;
pub mod harness;
pub mod id;
pub mod llm;
pub mod metrics;
pub mod planner;
pub mod recovery;
pub mod replay;
pub mod safety;
pub mod step;
pub mod task;

#[cfg(kani)]
mod kani_proofs;
