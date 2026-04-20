/// Output from a pipeline handler — value plus optional confidence score.
use serde_json::Value;

/// Carries the handler's output value and an optional confidence score.
///
/// Handlers that do not have a meaningful confidence score return `None`; the
/// runner treats that as `1.0` via [`HandlerOutput::confidence_or_default`].
#[derive(Debug, Clone)]
pub struct HandlerOutput {
    pub value: Value,
    pub confidence: Option<f32>,
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

    /// Returns the confidence score, defaulting to `1.0` when absent.
    pub fn confidence_or_default(&self) -> f32 {
        self.confidence.unwrap_or(1.0)
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
    use serde_json::json;

    #[test]
    fn from_value_has_no_confidence() {
        let out = HandlerOutput::from(json!({ "x": 1 }));
        assert!(out.confidence.is_none());
        assert_eq!(out.confidence_or_default(), 1.0);
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
        assert_eq!(out.confidence_or_default(), 1.0);
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
