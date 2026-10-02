//! Errors raised while validating a TypeSafe judgment.

use thiserror::Error;

/// Why a TypeSafe answer could not be turned into a routing confidence.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CalibrationError {
    /// The rubric has too few levels to normalize.
    ///
    /// A Score question requires at least two levels; with one there is no
    /// denominator and no spectrum to place the answer on.
    #[error("a Score rubric needs at least 2 levels to normalize, got {levels}")]
    TooFewLevels {
        /// Number of levels the backend reported.
        levels: usize,
    },

    /// The backend returned a non-finite `score`.
    ///
    /// Rejected rather than coerced. See the module docs of [`crate::calibration`]
    /// for why `0.0` and `NaN` are both worse than an error here.
    #[error("the judgment backend returned a non-finite score ({0})")]
    NonFiniteScore(f64),
}

impl CalibrationError {
    /// Attach the handler name to a diagnostic.
    pub fn step_failed(self, handler: &str) -> String {
        format!("{handler}: {self}")
    }
}
