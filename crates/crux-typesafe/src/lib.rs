#![warn(missing_docs)]
//! crux-typesafe — TypeSafe System One judgment handlers for crux pipelines.
//!
//! Provides `judge::score`, which rates some state against an ordered rubric and
//! reports a **calibrated** confidence that `route_on_confidence` can route on.
//!
//! # What this crate is for: calibration, not reachability
//!
//! YAML pipelines could route on confidence long before this crate existed.
//! `crux-baml` already declares `ConfidenceCapability::Always` on five handlers —
//! `llm::analyze`, `llm::confidence`, and every completion handler
//! (`llm::invoke`, `llm::invoke_with_fallback`, `llm::stream`) — and its own
//! documentation states that declaring it *"means any completion step can feed
//! `route_on_confidence` directly"*. `judge::score` declares `Always` too, but
//! that is table stakes, not the contribution.
//!
//! What those handlers cannot offer is a number that means one specific thing.
//! crux's existing confidence sources are either a model's answer to "how well is
//! this supported" — which `crux-baml` calls *"only loosely calibrated — a
//! confident-sounding answer is not necessarily a correct one"* — or, for the
//! `crux-stdlib` handler families (`ctrl::*`, `shell::*`, `fs::*`, `git::*`,
//! `text::*`, `json::*`), simply absent. Absence is not an error at read time:
//! `HandlerOutput::confidence_or_default` reports a neutral `0.5`, so a step that
//! measured nothing routes as though it were half-sure.
//!
//! A TypeSafe `Score` answer is a different kind of number. It carries an ordered
//! rubric of concrete situations (`criteria`), a probability-weighted position
//! along it (`score`), and the full distribution over levels (`probabilities`).
//! Normalizing that position onto `0..=1` puts a 3-level and a 4-level rubric on
//! the same axis, so a threshold means the same thing however many levels a
//! pipeline author wrote. The retry discount is deliberately kept identical to
//! `crux-baml`'s, so a score discounted here and one discounted there are
//! discounted the same way.
//!
//! # The mapping
//!
//! ```text
//! normalized = score / (levels - 1)          # positions the rubric on 0..=1
//! confidence = normalized * 0.8 ^ (attempts - 1)
//! ```
//!
//! # Example
//!
//! A YAML pipeline routes on the calibrated confidence. The schema below is the
//! real one: `route_on_confidence` requires a `routes:` list, and every entry
//! needs a `range:`, a `label:`, and a `handler:`. This example executes — it
//! loads the YAML with [`crux_script::load`], runs it through
//! [`crux_script::Runner`], and asserts which branch was taken. The same pipeline
//! is exercised at each band in `tests/routing.rs`.
//!
//! ```
//! use std::sync::Arc;
//!
//! use crux_runtime::prelude::CruxErr;
//! use crux_script::{HandlerRegistry, Runner, load};
//! use crux_typesafe::wire::{Answer, ScoreAnswer, SystemOneResponse};
//! use crux_typesafe::{CannedJudgmentClient, JudgmentClient};
//! use serde_json::{Value, json};
//!
//! // A three-level rubric, so the top level number is 2.
//! const PIPELINE: &str = r#"
//! pipeline: triage
//! steps:
//!   - step: classify
//!     handler: judge::score
//!     args:
//!       state: "The export button crashes the settings page in Safari."
//!       instructions: How severe is the reported issue?
//!       criteria:
//!         - Cosmetic; no impact
//!         - Degraded; workaround exists
//!         - Blocking; no workaround
//!   - route_on_confidence: act
//!     value: "{{ steps.classify.confidence }}"
//!     routes:
//!       - range: "[0.0, 0.34)"
//!         label: low
//!         handler: low
//!       - range: "[0.34, 0.67)"
//!         label: mid
//!         handler: mid
//!       - range: "[0.67, 1.0]"
//!         label: high
//!         handler: high
//! "#;
//!
//! // A canned judgment scoring 2.0 on the 0..=2 rubric. The handler
//! // implementation is swapped for a transport, so no key or network is needed.
//! fn judge_scoring_top() -> Arc<dyn JudgmentClient> {
//!     let response = SystemOneResponse {
//!         model: "jev-test".to_owned(),
//!         answers: [(
//!             "score".to_owned(),
//!             Answer::Score(ScoreAnswer {
//!                 score: 2.0,
//!                 legend: [
//!                     ("0".to_owned(), json!("Cosmetic; no impact")),
//!                     ("1".to_owned(), json!("Degraded; workaround exists")),
//!                     ("2".to_owned(), json!("Blocking; no workaround")),
//!                 ]
//!                 .into_iter()
//!                 .collect(),
//!                 probabilities: [("2".to_owned(), 1.0)].into_iter().collect(),
//!                 confidence: 1.0,
//!             }),
//!         )]
//!         .into_iter()
//!         .collect(),
//!         usage: None,
//!     };
//!     Arc::new(CannedJudgmentClient::empty().push(response))
//! }
//!
//! let mut registry = HandlerRegistry::new();
//! crux_typesafe::register(&mut registry, judge_scoring_top());
//!
//! // One handler per routing band, standing in for whatever the pipeline would
//! // really do in each.
//! for (name, out) in [("low", "low_out"), ("mid", "mid_out"), ("high", "high_out")] {
//!     registry.handler_value(name, move |_input: Value| async move {
//!         Ok::<Value, CruxErr>(json!(out))
//!     });
//! }
//!
//! let pipeline = load(PIPELINE).expect("the pipeline schema above must parse");
//! let crux = tokio::runtime::Runtime::new()
//!     .expect("a runtime")
//!     .block_on(Runner::new(Arc::new(registry)).run(&pipeline, json!(null)));
//!
//! // 2.0 on a 0..=2 rubric normalizes to 1.0, which lands in the top band. A raw
//! // 2.0 would not, which is what makes the normalization load-bearing.
//! assert_eq!(crux.value().expect("a value"), &json!("high_out"));
//! ```
//!
//! # Injection
//!
//! [`register`] takes a [`JudgmentClient`], which is the whole seam. Production
//! wires `HttpJudgmentClient`; tests wire [`CannedJudgmentClient`]. Nothing in
//! the handler or the calibration path needs an API key or network access, so the
//! routing above is asserted in CI against a canned response.
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
pub use wire::{Answer, ChoiceAnswer, NoulAnswer, ScoreAnswer, ScoreQuestion};

/// Register `judge::score` against a judgment backend.
///
/// `judge::score` declares `ConfidenceCapability::Always`, which is what makes
/// `{{ steps.<name>.confidence }}` resolvable from a YAML pipeline. Several
/// `crux-baml` handlers declare the same capability, so this is a contract the
/// crate satisfies rather than a capability it introduces.
///
/// # Examples
///
/// Register against a canned backend and read back the declared contract. No API
/// key and no network are involved.
///
/// ```
/// use std::sync::Arc;
///
/// use crux_script::{ConfidenceCapability, HandlerRegistry};
/// use crux_typesafe::CannedJudgmentClient;
///
/// let mut registry = HandlerRegistry::new();
/// crux_typesafe::register(
///     &mut registry,
///     Arc::new(CannedJudgmentClient::empty()),
/// );
///
/// let metadata = registry
///     .get_metadata("judge::score")
///     .expect("judge::score must be registered");
/// assert_eq!(metadata.confidence, Some(ConfidenceCapability::Always));
/// ```
///
/// To serve the live backend instead, pass an `HttpJudgmentClient` (available
/// with the default `http` feature) in place of the canned client.
pub use score::register;

pub use score::register_score;

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
