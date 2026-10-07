//! Errors raised while validating a TypeSafe judgment.

use thiserror::Error;

/// Why a TypeSafe answer could not be turned into a routing confidence.
///
/// `#[non_exhaustive]` so a variant added later is not a breaking change for a
/// downstream `match`. It is free while the crate is unpublished and expensive to
/// add afterwards (finding #15a).
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum CalibrationError {
    /// The rubric has too few levels to normalize.
    ///
    /// A Score question requires at least two levels; with one there is no
    /// denominator and no spectrum to place the answer on.
    #[error("a Score rubric needs at least 2 levels to normalize, got {levels}")]
    TooFewLevels {
        /// Number of levels the request asked for.
        levels: usize,
    },

    /// The backend returned a non-finite `score`.
    ///
    /// Rejected rather than coerced. See the module docs of [`crate::calibration`]
    /// for why `0.0` and `NaN` are both worse than an error here.
    #[error("the judgment backend returned a non-finite score ({0})")]
    NonFiniteScore(f64),
}

// No `step_failed` helper: `CruxErr::step_failed` already prefixes the step name,
// so a helper doing the same produced a doubled prefix (finding #9). Render with
// `error.to_string()` at the call site.
