//! Turning a TypeSafe answer into a routing confidence.
//!
//! # Why this module exists
//!
//! `HandlerOutput::confidence` feeds `route_on_confidence`, which routes to the
//! first range containing the score. The score therefore has to mean *one*
//! thing, or a threshold stops being interpretable.
//!
//! For a BAML-backed step that number is
//! `self_report * RETRY_DECAY ^ (attempts - 1)` — a model's own answer to "how
//! well is this supported", discounted by retries. `crux-baml` documents plainly
//! that the self-report half is *"only loosely calibrated — a confident-sounding
//! answer is not necessarily a correct one"*.
//!
//! A TypeSafe `Score` answer carries something different and better: an ordered
//! rubric of *situations* (`criteria`), a probability-weighted position along it
//! (`score`), and the full distribution over levels (`probabilities`).
//!
//! # The mapping
//!
//! ```text
//! normalized = score / (levels - 1)          # positions the rubric on 0..=1
//! confidence = normalized * RETRY_DECAY ^ (attempts - 1)
//! ```
//!
//! The division puts an N-level rubric on the same 0..=1 axis as a 2-level one,
//! so a threshold means the same thing regardless of how many levels a pipeline
//! author wrote. The retry term is kept identical to [`crux_baml`]'s, so a score
//! discounted here and one discounted there are discounted the same way.
//!
//! # What is deliberately *not* used
//!
//! TypeSafe's `confidence` field is **not** the routing value. It measures how
//! concentrated `probabilities` is — all weight on one level gives `1.0`. Feeding
//! it to `route_on_confidence` would silently swap the meaning of the number from
//! *"how well supported is this"* to *"how decisive was the judgment"*, and a
//! pipeline that split its probability evenly across two equally good outcomes
//! would route as though nothing were known. Distribution concentration is
//! surfaced in the payload as `distribution_confidence` instead, where a template
//! or a human can read it without it silently steering a route.
//!
//! # Rejections
//!
//! Unlike [`crux_baml::confidence::CallEvidence`], which scores a non-finite
//! self-report as `0.0`, this module rejects one. `HandlerOutput::with_confidence`
//! turns `NaN` into `None`, and `confidence_or_default` then reports a neutral
//! `0.5` — so a malformed answer would flow into routing as a confident-looking
//! midpoint with nothing wrong on the wire to explain it. An error is the honest
//! outcome. `0.0` is worse still: it means "maximally unsupported" and would route
//! to the lowest band as though the pipeline had reason to distrust itself.
//!
//! [`crux_baml::confidence::CallEvidence`]: https://docs.rs/crux-baml

use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::CalibrationError;
use crate::wire::ScoreAnswer;

/// Discount applied per extra attempt the judgment backend needed.
///
/// Kept numerically identical to `crux_baml`'s retry term so the two confidence
/// sources discount the same way. At 0.8 the sequence for 1/2/3/4 attempts is
/// 1.0, 0.80, 0.64, 0.51 — a retry is meaningful evidence, not disqualifying.
pub const RETRY_DECAY: f64 = 0.8;

/// A TypeSafe `Score` answer plus the evidence needed to calibrate it.
///
/// `levels` is supplied by the caller from the **request's** `criteria`, not read
/// back off the answer. The API echoes a `legend` describing the levels, but a
/// response is not the authority on what was asked: if an echoed legend disagreed
/// with `criteria`, the same judgment would normalize differently and route
/// differently (finding #8).
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreEvidence {
    /// The answer as returned by the judgment backend.
    ///
    /// `legend` is carried for context only; it does not affect calibration.
    pub answer: ScoreAnswer,
    /// How many levels the rubric has, from the request's `criteria.len()`.
    pub levels: usize,
    /// How many backend calls were needed, including retries.
    ///
    /// Zero is treated as one (no penalty), matching
    /// `crux_baml::confidence::CallEvidence::new`.
    ///
    /// No production path can currently observe a value above one; see the
    /// `FIXME(typesafe-followup)` on `score::OBSERVED_ATTEMPTS` (finding #3).
    pub attempts: u32,
}

impl ScoreEvidence {
    /// Pair an answer with the rubric width the caller asked for.
    pub fn new(answer: ScoreAnswer, levels: usize, attempts: u32) -> Self {
        Self {
            answer,
            levels,
            attempts: attempts.max(1),
        }
    }

    /// The retry discount for this call.
    pub fn retry_multiplier(&self) -> f64 {
        RETRY_DECAY.powi(self.attempts.saturating_sub(1) as i32)
    }

    /// The rubric position normalized onto `0.0..=1.0`.
    ///
    /// A top-level score on an N-level rubric is `(N-1)`, so dividing by the top
    /// level number is what makes a 3-level and a 4-level rubric comparable. `N`
    /// comes from the request's `criteria`, never from the echoed `legend`.
    ///
    /// # Errors
    ///
    /// Returns [`CalibrationError::TooFewLevels`] when the rubric cannot be
    /// normalized (a Score requires at least two levels), and
    /// [`CalibrationError::NonFiniteScore`] when the backend returned a
    /// non-finite `score`.
    pub fn normalized(&self) -> Result<f64, CalibrationError> {
        let top = self.top_level()?;
        let score = self.answer.score;
        if !score.is_finite() {
            return Err(CalibrationError::NonFiniteScore(score));
        }
        // A score is mathematically bounded by the level count, but floating
        // point accumulation of `sum(level * probability)` can land a hair
        // outside. Clamping keeps a well-formed answer from failing on drift.
        Ok((score / top).clamp(0.0, 1.0))
    }

    /// The calibrated confidence to attach to a `HandlerOutput`.
    ///
    /// Always finite and within `0.0..=1.0`, which is exactly what
    /// `HandlerOutput::with_confidence` and `route_on_confidence` require.
    ///
    /// # Errors
    ///
    /// Propagates [`ScoreEvidence::normalized`]'s errors.
    pub fn confidence(&self) -> Result<f32, CalibrationError> {
        let normalized = self.normalized()?;
        Ok((normalized * self.retry_multiplier()).clamp(0.0, 1.0) as f32)
    }

    /// Highest level number in the rubric.
    ///
    /// Derived from the request's `criteria`, which `levels` records.
    fn top_level(&self) -> Result<f64, CalibrationError> {
        let top = self.levels as f64 - 1.0;
        if top < 1.0 {
            return Err(CalibrationError::TooFewLevels {
                levels: self.levels,
            });
        }
        Ok(top)
    }
}

/// The JSON payload a judgment step returns.
///
/// # Reserved keys
///
/// `confidence` is absent by design. `crux-baml::schema_bridge` reserves it for
/// the step confidence because a payload that carried its own `confidence` could
/// disagree with the routed value, and routing must not have two sources of
/// truth. TypeSafe's own concentration metric is published here as
/// `distribution_confidence` instead.
#[derive(Debug, Clone, PartialEq)]
pub struct JudgmentPayload {
    /// The raw `score` reported by the backend, before normalization.
    pub score: f64,
    /// `score` divided by the top level number, on `0.0..=1.0`.
    pub normalized: f64,
    /// Probability of each level, keyed by level number.
    pub probabilities: BTreeMap<String, f64>,
    /// How concentrated `probabilities` is — TypeSafe's `confidence`.
    ///
    /// Descriptive only. This never feeds `route_on_confidence`.
    pub distribution_confidence: f64,
    /// Backend calls needed, including retries.
    pub attempts: u32,
    /// The discount applied for those attempts.
    pub retry_multiplier: f64,
}

impl JudgmentPayload {
    /// Render as the handler's JSON output value.
    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "score": self.score,
            "normalized": self.normalized,
            "probabilities": self.probabilities,
            "distribution_confidence": self.distribution_confidence,
            "attempts": self.attempts,
            "retry_multiplier": self.retry_multiplier,
        })
    }
}

/// Calibrate a Score answer into both a payload and a routing confidence.
///
/// The two are produced together because the payload's `normalized` field and
/// the routing confidence must describe the same arithmetic — computing them
/// separately is how they drift apart.
pub fn calibrate_score(evidence: &ScoreEvidence) -> Result<(Value, f32), CalibrationError> {
    let normalized = evidence.normalized()?;
    // Delegated, not recomputed: this function's own doc comment promises the
    // payload and the routing value describe the same arithmetic, and an inline
    // copy of `normalized * retry_multiplier` is exactly how they would drift
    // apart (finding #12).
    let confidence = evidence.confidence()?;

    let payload = JudgmentPayload {
        score: evidence.answer.score,
        normalized,
        probabilities: evidence.answer.probabilities.clone(),
        distribution_confidence: evidence.answer.confidence,
        attempts: evidence.attempts,
        retry_multiplier: evidence.retry_multiplier(),
    };

    Ok((payload.to_value(), confidence))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::ScoreAnswer;

    fn answer(levels: usize, score: f64) -> ScoreAnswer {
        let mut legend = BTreeMap::new();
        let mut probabilities = BTreeMap::new();
        for level in 0..levels {
            legend.insert(level.to_string(), Value::from(format!("level {level}")));
            probabilities.insert(level.to_string(), 0.0);
        }
        ScoreAnswer {
            score,
            legend,
            probabilities,
            confidence: 0.5,
        }
    }

    /// The ordinary case: the echoed legend agrees with the requested width.
    fn evidence(levels: usize, score: f64, attempts: u32) -> ScoreEvidence {
        ScoreEvidence::new(answer(levels, score), levels, attempts)
    }

    /// Pair an arbitrary answer with a rubric width the caller chose independently
    /// of the answer's legend width.
    fn evidence_with(answer: ScoreAnswer, levels: usize, attempts: u32) -> ScoreEvidence {
        ScoreEvidence::new(answer, levels, attempts)
    }

    #[test]
    fn a_top_level_score_normalizes_to_one() {
        let evidence = evidence(3, 2.0, 1);
        assert!((evidence.normalized().unwrap() - 1.0).abs() < 1e-9);
        assert!((evidence.confidence().unwrap() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_bottom_level_score_normalizes_to_zero() {
        let evidence = evidence(3, 0.0, 1);
        assert_eq!(evidence.normalized().unwrap(), 0.0);
        assert_eq!(evidence.confidence().unwrap(), 0.0);
    }

    /// The point of normalizing: the same rubric position must give the same
    /// confidence regardless of how many levels the pipeline author wrote.
    #[test]
    fn rubrics_of_different_width_normalize_identically() {
        let three = evidence(3, 1.0, 1); // midpoint of 0..2
        let four = evidence(4, 1.5, 1); // midpoint of 0..3
        assert!((three.normalized().unwrap() - 0.5).abs() < 1e-9);
        assert!((four.normalized().unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_fractional_position_is_preserved() {
        let evidence = evidence(3, 1.43, 1);
        assert!((evidence.normalized().unwrap() - 0.715).abs() < 1e-9);
    }

    /// Retry arithmetic, as a unit contract.
    ///
    /// Finding #3: no production path can supply `attempts > 1` yet, because the
    /// judgment port does not surface a retry count. This pins the arithmetic that
    /// *will* apply once it does; the production behaviour is pinned separately by
    /// `score::tests::the_handler_reports_one_attempt_while_the_port_hides_retries`.
    #[test]
    fn retries_discount_the_score() {
        let base = evidence(3, 2.0, 1).confidence().unwrap();
        let once = evidence(3, 2.0, 2).confidence().unwrap();
        let twice = evidence(3, 2.0, 3).confidence().unwrap();
        assert!(once < base);
        assert!(twice < once);
        assert!((once - 0.8).abs() < 1e-6, "got {once}");
        assert!((twice - 0.64).abs() < 1e-6, "got {twice}");
    }

    /// Retry evidence is objective: a low rubric position stays low however many
    /// attempts it took, and a retry can never rescue it into confidence.
    #[test]
    fn retries_cannot_rescue_a_bottom_level_score() {
        let evidence = evidence(3, 0.0, 5);
        assert_eq!(evidence.confidence().unwrap(), 0.0);
    }

    #[test]
    fn zero_attempts_is_treated_as_one() {
        let evidence = evidence(3, 1.0, 0);
        assert_eq!(evidence.attempts, 1);
        assert!((evidence.confidence().unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_single_level_rubric_cannot_be_normalized() {
        let error = evidence(1, 0.0, 1).normalized().unwrap_err();
        assert!(matches!(
            error,
            CalibrationError::TooFewLevels { levels: 1 }
        ));
    }

    /// Non-finite scores must surface as errors, never as a routing number.
    #[test]
    fn non_finite_scores_are_rejected_rather_than_scored() {
        for score in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let evidence = evidence(3, score, 1);
            assert!(
                matches!(
                    evidence.normalized(),
                    Err(CalibrationError::NonFiniteScore(_))
                ),
                "score {score} must be rejected"
            );
        }
    }

    /// Floating point drift in `sum(level * probability)` must not fail a
    /// well-formed answer.
    #[test]
    fn a_score_a_hair_above_the_top_level_is_clamped() {
        let evidence = evidence(3, 2.000_000_1, 1);
        assert!((evidence.normalized().unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_result_is_always_finite_and_in_range() {
        for levels in 2..=10 {
            for raw in [0.0, 0.5, 1.0, 5.0, -5.0] {
                for attempts in [1, 2, 3, 10] {
                    let score = raw * (levels as f64 - 1.0);
                    let confidence = evidence(levels, score, attempts).confidence().unwrap();
                    assert!(
                        confidence.is_finite() && (0.0..=1.0).contains(&confidence),
                        "levels={levels} score={score} attempts={attempts} produced {confidence}"
                    );
                }
            }
        }
    }

    /// Finding #8: the normalization denominator comes from the request's
    /// `criteria`, not the response's echoed `legend`.
    ///
    /// Before the fix this read `legend.len()`, so a backend that echoed a legend
    /// disagreeing with the request silently rescaled the same judgment — from
    /// 0.5 (3 requested levels) to 0.125 (9 echoed levels) — and a pipeline's
    /// thresholds would stop meaning what the author wrote.
    #[test]
    fn a_mismatched_echoed_legend_cannot_change_the_result() {
        // 3 requested levels, legend agrees: 1.0 of 0..=2 is the midpoint.
        let honest = evidence(3, 1.0, 1);
        // Identical judgment and identical request, but the backend echoed 9 levels.
        let tampered = evidence_with(answer(9, 1.0), 3, 1);

        assert!((honest.normalized().unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(
            tampered.normalized().unwrap(),
            honest.normalized().unwrap(),
            "an echoed legend of a different width must not rescale the judgment"
        );
        assert_eq!(tampered.confidence().unwrap(), honest.confidence().unwrap());
    }

    /// A narrower echo is just as untrustworthy as a wider one.
    #[test]
    fn a_narrower_echoed_legend_cannot_change_the_result() {
        let honest = evidence(3, 1.0, 1);
        let tampered = evidence_with(answer(2, 1.0), 3, 1);

        assert!((tampered.normalized().unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(tampered.normalized().unwrap(), honest.normalized().unwrap());
    }

    /// Finding #12: `calibrate_score` must not recompute the confidence. The
    /// payload's `normalized` and the returned routing value have to be the same
    /// arithmetic, so they are asserted equal for every attempt count — which is
    /// what an inline copy of the formula would eventually stop satisfying.
    #[test]
    fn the_routed_confidence_is_exactly_the_evidence_confidence() {
        for levels in 2..=6 {
            for attempts in [1, 2, 3, 7] {
                for score in [0.0, 1.0, (levels as f64 - 1.0) / 2.0] {
                    let evidence = evidence(levels, score, attempts);
                    let expected = evidence.confidence().unwrap();
                    let (_, confidence) = calibrate_score(&evidence).unwrap();
                    assert_eq!(
                        confidence, expected,
                        "levels={levels} score={score} attempts={attempts}"
                    );
                }
            }
        }
    }

    /// The payload must not shadow the routing value.
    #[test]
    fn the_payload_never_carries_a_confidence_key() {
        let (value, _) = calibrate_score(&evidence(3, 1.0, 1)).unwrap();
        let object = value.as_object().unwrap();
        assert!(
            !object.contains_key("confidence"),
            "payload must not define 'confidence'; it would disagree with the step confidence"
        );
        assert!(object.contains_key("distribution_confidence"));
        assert!(object.contains_key("normalized"));
    }

    /// TypeSafe's own concentration metric is reported, but never routed on.
    #[test]
    fn distribution_confidence_is_reported_verbatim() {
        let mut raw = answer(3, 1.0);
        raw.confidence = 0.35;
        let (value, confidence) = calibrate_score(&evidence_with(raw, 3, 1)).unwrap();
        assert_eq!(value["distribution_confidence"], serde_json::json!(0.35));
        // 1.0 of 2.0 normalized, unaffected by distribution concentration.
        assert!((confidence - 0.5).abs() < 1e-6);
    }

    /// A concentrated distribution and a spread one at the same rubric position
    /// must route identically — that is the whole reason `distribution_confidence`
    /// is not the routing input.
    #[test]
    fn distribution_concentration_does_not_change_the_route() {
        let mut concentrated = answer(3, 1.0);
        concentrated.confidence = 1.0;
        let mut spread = answer(3, 1.0);
        spread.confidence = 0.05;

        let (_, a) = calibrate_score(&evidence_with(concentrated, 3, 1)).unwrap();
        let (_, b) = calibrate_score(&evidence_with(spread, 3, 1)).unwrap();
        assert_eq!(a, b);
    }

    /// Probabilities reach the payload so a template can read the distribution.
    #[test]
    fn the_payload_carries_the_distribution() {
        let mut raw = answer(3, 1.0);
        raw.probabilities = BTreeMap::from([
            ("0".to_owned(), 0.1),
            ("1".to_owned(), 0.6),
            ("2".to_owned(), 0.3),
        ]);
        let (value, _) = calibrate_score(&evidence_with(raw, 3, 1)).unwrap();
        assert_eq!(value["probabilities"]["1"], serde_json::json!(0.6));
        assert_eq!(value["attempts"], serde_json::json!(1));
    }
}
