//! Confidence scoring for BAML-backed LLM calls.
//!
//! Every handler in this crate returns a score, so any LLM step can feed
//! `route_on_confidence`. The number is not invented — it is the model's own
//! self-report, adjusted by objective evidence BAML exposes about the call.
//!
//! # Where the number comes from
//!
//! Two inputs:
//!
//! 1. **Self-report.** Every BAML completion type declares a `confidence` field,
//!    and the prompt asks the model how well the answer is supported. This is the
//!    model's own judgement, not a claim about the world's ground truth.
//! 2. **Retry penalty.** BAML's `FunctionLog::calls()` returns "all calls made
//!    (including retries)". Needing several attempts to produce a parseable
//!    answer is real evidence that the first answers were wrong, so each extra
//!    attempt discounts the score.
//!
//! ```text
//! confidence = self_report * RETRY_DECAY ^ (attempts - 1)
//! ```
//!
//! A first-try success passes the self-report through untouched.
//!
//! # Calibration caveat
//!
//! Self-reported confidence from a language model is only loosely calibrated —
//! a confident-sounding answer is not necessarily a correct one. The retry term
//! is the objective half of this score; the self-report is the subjective half.
//! For decisions that need real calibration, put the claim through
//! [`llm::confidence`](super::confidence_scoring_note), which scores support
//! against explicit evidence and criteria rather than asking the model to grade
//! itself.

/// Discount applied per extra attempt BAML needed to reach a parseable answer.
///
/// A retry means the previous output failed validation, so the model struggled.
/// At 0.8 the sequence for 1/2/3/4 attempts is 1.0, 0.80, 0.64, 0.51 — a retry is
/// treated as meaningful but not disqualifying.
const RETRY_DECAY: f64 = 0.8;

/// The observations a single BAML call contributes to a confidence score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CallEvidence {
    /// How many times BAML called the model, including retries.
    pub attempts: u32,
    /// The confidence the model reported in its `confidence` field.
    pub self_report: f64,
}

impl CallEvidence {
    /// Build evidence from a BAML self-report and a call-attempt count.
    ///
    /// A zero attempt count means no call was recorded, which is treated as one
    /// attempt (no penalty) rather than as maximum distrust.
    pub fn new(self_report: f64, attempts: u32) -> Self {
        Self {
            attempts: attempts.max(1),
            self_report,
        }
    }

    /// The retry discount for this call.
    pub fn retry_multiplier(&self) -> f64 {
        RETRY_DECAY.powi(self.attempts.saturating_sub(1) as i32)
    }

    /// The combined confidence, clamped into `[0.0, 1.0]`.
    ///
    /// A non-finite self-report (a model emitting `NaN` is possible) scores 0.0
    /// rather than propagating into `HandlerOutput::with_confidence`, which
    /// rejects NaN outright.
    pub fn confidence(&self) -> f32 {
        if !self.self_report.is_finite() {
            return 0.0;
        }
        let combined = self.self_report.clamp(0.0, 1.0) * self.retry_multiplier();
        combined.clamp(0.0, 1.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_attempt_passes_the_self_report_through() {
        let evidence = CallEvidence::new(0.8, 1);
        assert_eq!(evidence.retry_multiplier(), 1.0);
        assert!((evidence.confidence() - 0.8).abs() < 1e-6);
    }

    #[test]
    fn each_retry_discounts_the_score() {
        let base = 1.0;
        let scores: Vec<f32> = (1..=4)
            .map(|attempts| CallEvidence::new(base, attempts).confidence())
            .collect();
        for pair in scores.windows(2) {
            assert!(
                pair[1] < pair[0],
                "scores must fall as attempts rise: {scores:?}"
            );
        }
        // 1.0, 0.80, 0.64, 0.51
        assert!((scores[1] - 0.8).abs() < 1e-6, "got {scores:?}");
        assert!((scores[2] - 0.64).abs() < 1e-6, "got {scores:?}");
    }

    #[test]
    fn a_low_self_report_stays_low_however_many_retries() {
        assert!(CallEvidence::new(0.2, 1).confidence() < 0.3);
        assert!(CallEvidence::new(0.2, 5).confidence() < 0.1);
    }

    #[test]
    fn a_high_self_report_survives_one_retry() {
        assert!(CallEvidence::new(1.0, 2).confidence() > 0.75);
    }

    #[test]
    fn out_of_range_self_reports_are_clamped() {
        assert_eq!(CallEvidence::new(1.4, 1).confidence(), 1.0);
        assert_eq!(CallEvidence::new(-0.3, 1).confidence(), 0.0);
    }

    #[test]
    fn a_non_finite_self_report_scores_zero() {
        assert_eq!(CallEvidence::new(f64::NAN, 1).confidence(), 0.0);
        assert_eq!(CallEvidence::new(f64::INFINITY, 1).confidence(), 0.0);
    }

    #[test]
    fn zero_attempts_is_treated_as_one() {
        assert_eq!(CallEvidence::new(0.9, 0).attempts, 1);
        assert!((CallEvidence::new(0.9, 0).confidence() - 0.9).abs() < 1e-6);
    }

    /// The score must always be a finite value in range, because
    /// `HandlerOutput::with_confidence` rejects anything else.
    #[test]
    fn the_score_is_always_usable_as_a_step_confidence() {
        for self_report in [0.0, 0.5, 1.0, 2.0, -1.0, f64::NAN] {
            for attempts in [1, 2, 3, 10] {
                let score = CallEvidence::new(self_report, attempts).confidence();
                assert!(
                    score.is_finite() && (0.0..=1.0).contains(&score),
                    "self_report={self_report} attempts={attempts} produced {score}"
                );
            }
        }
    }
}
