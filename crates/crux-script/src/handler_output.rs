use crux_runtime::prelude::{CruxErr, HandlerUsage};
/// Output from a pipeline handler — value plus optional confidence score.
use serde_json::Value;

/// Carries the handler's output value and an optional confidence score.
///
/// Handlers that do not have a meaningful confidence score return `None`. Previously
/// [`HandlerOutput::confidence_or_default`] silently treated that as `1.0`, which made
/// unscored handlers look maximally confident to any consumer relying on the default
/// (e.g. `route_on_confidence`). To avoid that false signal, `None` now defaults to
/// `0.5` (a neutral midpoint) instead of `1.0`. This is a behavior change but is less
/// invasive than making every `None`-confidence caller handle a hard error, since the
/// only in-crate callers of this method were tests (see #75, #76).
#[derive(Debug, Clone)]
pub struct HandlerOutput {
    pub value: Value,
    pub confidence: Option<f32>,
}

/// A handler outcome paired with usage reported for the invocation.
#[derive(Debug, Clone)]
pub struct HandlerExecution {
    pub outcome: Result<HandlerOutput, CruxErr>,
    pub usage: HandlerUsage,
}

impl HandlerExecution {
    pub fn success(output: HandlerOutput, usage: HandlerUsage) -> Self {
        Self {
            outcome: Ok(output),
            usage,
        }
    }

    pub fn failure(error: CruxErr, usage: HandlerUsage) -> Self {
        Self {
            outcome: Err(error),
            usage,
        }
    }

    pub fn free(outcome: Result<HandlerOutput, CruxErr>) -> Self {
        Self {
            outcome,
            usage: HandlerUsage::free(),
        }
    }

    pub fn unreported(outcome: Result<HandlerOutput, CruxErr>) -> Self {
        Self {
            outcome,
            usage: HandlerUsage::unreported(),
        }
    }

    pub fn is_ok(&self) -> bool {
        self.outcome.is_ok()
    }

    pub fn is_err(&self) -> bool {
        self.outcome.is_err()
    }

    pub fn unwrap(self) -> HandlerOutput {
        self.outcome.unwrap()
    }

    pub fn unwrap_err(self) -> CruxErr {
        self.outcome.unwrap_err()
    }

    pub fn expect_err(self, message: &str) -> CruxErr {
        self.outcome.expect_err(message)
    }
}

impl HandlerOutput {
    pub fn new(value: Value) -> Self {
        Self {
            value,
            confidence: None,
        }
    }

    /// Constructs a `HandlerOutput` with a validated confidence score.
    ///
    /// - NaN is treated as absent confidence (`None`).
    /// - Values outside `[0.0, 1.0]` are clamped to the nearest bound.
    pub fn with_confidence(value: Value, confidence: f32) -> Self {
        let confidence = if confidence.is_nan() {
            None
        } else {
            Some(confidence.clamp(0.0, 1.0))
        };
        Self { value, confidence }
    }

    /// Returns the confidence score, defaulting to `0.5` (neutral) when absent.
    ///
    /// Prior to #76 this defaulted to `1.0`, which silently made unscored handlers
    /// look maximally confident. `0.5` signals "unknown" without biasing routing
    /// decisions toward either extreme.
    pub fn confidence_or_default(&self) -> f32 {
        self.confidence.unwrap_or(0.5)
    }
}

impl std::ops::Deref for HandlerOutput {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl PartialEq<Value> for HandlerOutput {
    fn eq(&self, other: &Value) -> bool {
        &self.value == other
    }
}

impl From<Value> for HandlerOutput {
    fn from(value: Value) -> Self {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crux_runtime::prelude::{HandlerUsage, UsdAmount};
    use serde_json::json;

    #[test]
    fn from_value_has_no_confidence() {
        let out = HandlerOutput::from(json!({ "x": 1 }));
        assert!(out.confidence.is_none());
        assert_eq!(out.confidence_or_default(), 0.5);
    }

    #[test]
    fn execution_preserves_usage_for_success_and_failure() {
        let usage = HandlerUsage::metered(10, UsdAmount::from_micros(25));
        let success = HandlerExecution::success(HandlerOutput::new(json!(1)), usage);
        let failure = HandlerExecution::failure(CruxErr::step_failed("x", "boom"), usage);

        assert_eq!(success.usage, usage);
        assert_eq!(failure.usage, usage);
        assert!(success.outcome.is_ok());
        assert!(failure.outcome.is_err());
    }

    /// Regression test for #76: `None` confidence must NOT silently present as
    /// maximal (`1.0`) confidence — it should default to a neutral `0.5`.
    #[test]
    fn none_confidence_defaults_to_neutral_not_maximal() {
        let out = HandlerOutput::new(json!("unscored"));
        assert_eq!(
            out.confidence_or_default(),
            0.5,
            "None confidence must default to neutral 0.5, not maximal 1.0 (#76)"
        );
    }

    #[test]
    fn with_confidence_stores_score() {
        let out = HandlerOutput::with_confidence(json!("ok"), 0.75);
        assert_eq!(out.confidence, Some(0.75));
        assert_eq!(out.confidence_or_default(), 0.75);
    }

    #[test]
    fn nan_confidence_becomes_none() {
        let out = HandlerOutput::with_confidence(json!("x"), f32::NAN);
        assert!(out.confidence.is_none());
        assert_eq!(out.confidence_or_default(), 0.5);
    }

    #[test]
    fn negative_confidence_clamped_to_zero() {
        let out = HandlerOutput::with_confidence(json!("x"), -0.5);
        assert_eq!(out.confidence, Some(0.0));
    }

    #[test]
    fn confidence_above_one_clamped_to_one() {
        let out = HandlerOutput::with_confidence(json!("x"), 1.5);
        assert_eq!(out.confidence, Some(1.0));
    }

    #[test]
    fn boundary_values_accepted_as_is() {
        let lo = HandlerOutput::with_confidence(json!("x"), 0.0);
        let hi = HandlerOutput::with_confidence(json!("x"), 1.0);
        assert_eq!(lo.confidence, Some(0.0));
        assert_eq!(hi.confidence, Some(1.0));
    }

    #[test]
    fn new_is_same_as_from() {
        let v = json!(42);
        let a = HandlerOutput::new(v.clone());
        let b = HandlerOutput::from(v);
        assert!(a.confidence.is_none());
        assert!(b.confidence.is_none());
    }
}
