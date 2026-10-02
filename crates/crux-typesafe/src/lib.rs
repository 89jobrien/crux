#![warn(missing_docs)]
//! crux-typesafe — TypeSafe System One judgment handlers for crux pipelines.
//!
//! Provides `judge::score`, which rates some state against an ordered rubric and
//! reports a **calibrated** confidence that `route_on_confidence` can route on.
//!
//! # Why this exists
//!
//! Every other confidence source in crux is either an objective retry count or a
//! model's own estimate of how well it did. `crux-baml` documents that its
//! self-report half is *"only loosely calibrated"*. `HandlerOutput` defaults an
//! absent score to a neutral `0.5`, so an unscored step routing as though it knew
//! something is a real hazard.
//!
//! A TypeSafe `Score` is a rubric of concrete situations plus a distribution over
//! them. Normalizing its position onto `0..=1` gives a number that means the same
//! thing regardless of how many levels the rubric has, and keeping the retry
//! discount preserves the objective evidence crux already had.
//!
//! # Routing from YAML
//!
//! `ConfidenceCapability::Always` is the part that matters most. A YAML pipeline
//! cannot register a Rust handler, so before this crate **no YAML pipeline could
//! route on confidence at all** — `crux run --check` rejects it as
//! `dynamic_boundary: step '<name>' may not report confidence`, which is a real
//! defect in `examples/joe/branch_cleanup.crux`. A registered handler that
//! declares `Always` lifts that restriction.
//!
//! ```text
//! - step: judge
//!   handler: judge::score
//!   args:
//!     state: "{{ steps.intake.output.text }}"
//!     instructions: How severe is this report?
//!     criteria:
//!       - Cosmetic; no impact
//!       - Degraded but a workaround exists
//!       - Blocking; no workaround
//! - route_on_confidence: act
//!   value: "{{ steps.judge.confidence }}"
//! ```
//!
//! # Injection
//!
//! [`register`] takes a [`JudgmentClient`]. Production passes
//! [`HttpJudgmentClient`]; tests pass [`CannedJudgmentClient`]. Nothing here
//! requires an API key or network access at test time, which is what
//! `CONFORMANCE.md`'s rule requires.
//!
//! # What is deliberately not used
//!
//! TypeSafe's own `confidence` field measures how *concentrated* a distribution
//! is, not how correct the answer is. It is reported as
//! `distribution_confidence` in the payload and never routed on — see
//! [`calibration`].

pub mod calibration;
pub mod client;
pub mod error;
pub mod score;
pub mod wire;

pub use calibration::{JudgmentPayload, ScoreEvidence, calibrate_score};
pub use client::{CannedJudgmentClient, ClientError, JudgmentClient, RetryPolicy};
pub use error::CalibrationError;
pub use score::{register, register_score};
pub use wire::{Answer, ChoiceAnswer, NoulAnswer, ScoreAnswer, ScoreQuestion};

#[cfg(feature = "http")]
pub use client::HttpJudgmentClient;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_with_a_canned_client_does_not_panic() {
        let mut registry = crux_script::HandlerRegistry::new();
        register(
            &mut registry,
            std::sync::Arc::new(CannedJudgmentClient::empty()),
        );
    }
}
