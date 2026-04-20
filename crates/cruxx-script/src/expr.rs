/// Minimal expression evaluator for `{{ path }}` references in YAML values.
///
/// Supports: `{{ input }}`, `{{ steps.<name>.output }}`, `{{ steps.<name>.confidence }}`.
use serde_json::Value;
use std::collections::HashMap;

/// Result of a completed step, used for expression resolution.
///
/// `confidence` is `None` for steps that produced no score (e.g. `handler_value` handlers).
/// Routing steps (`route_on_confidence`) that reference such a step will receive an
/// [`ExprError::NoConfidence`] rather than a spurious `1.0`.
#[derive(Debug, Clone)]
pub struct StepResult {
    pub output: Value,
    pub confidence: Option<f32>,
}

/// Evaluation context holding pipeline state.
pub struct ExprContext {
    pub input: Value,
    pub steps: HashMap<String, StepResult>,
}

impl ExprContext {
    pub fn new(input: Value) -> Self {
        Self {
            input,
            steps: HashMap::new(),
        }
    }

    /// Evaluate an expression string. Returns the resolved Value.
    ///
    /// If the string is `{{ path }}`, resolves it. Otherwise returns
    /// the string as a JSON string value.
    pub fn eval(&self, expr: &str) -> Result<Value, ExprError> {
        let trimmed = expr.trim();
        if let Some(path) = trimmed.strip_prefix("{{") {
            let path = path
                .strip_suffix("}}")
                .ok_or_else(|| ExprError::Syntax(expr.to_string()))?;
            self.resolve_path(path.trim())
        } else {
            Ok(Value::String(expr.to_string()))
        }
    }

    /// Evaluate an expression to f32 (for confidence routing).
    pub fn eval_f32(&self, expr: &str) -> Result<f32, ExprError> {
        let value = self.eval(expr)?;
        match value {
            Value::Number(n) => n.as_f64().map(|f| f as f32).ok_or(ExprError::NotNumeric),
            _ => Err(ExprError::NotNumeric),
        }
    }

    fn resolve_path(&self, path: &str) -> Result<Value, ExprError> {
        if path == "input" {
            return Ok(self.input.clone());
        }

        let parts: Vec<&str> = path.splitn(3, '.').collect();
        match parts.as_slice() {
            ["steps", name, "output"] => self
                .steps
                .get(*name)
                .map(|r| r.output.clone())
                .ok_or_else(|| ExprError::UnknownStep((*name).to_string())),
            ["steps", name, "confidence"] => {
                let result = self
                    .steps
                    .get(*name)
                    .ok_or_else(|| ExprError::UnknownStep((*name).to_string()))?;
                let score = result
                    .confidence
                    .ok_or_else(|| ExprError::NoConfidence((*name).to_string()))?;
                Ok(Value::Number(
                    serde_json::Number::from_f64(score as f64).unwrap(),
                ))
            }
            _ => Err(ExprError::UnknownPath(path.to_string())),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExprError {
    #[error("syntax error in expression: {0}")]
    Syntax(String),
    #[error("unknown step: {0}")]
    UnknownStep(String),
    #[error("unknown path: {0}")]
    UnknownPath(String),
    #[error("value is not numeric")]
    NotNumeric,
    /// Step exists but produced no confidence score (e.g. a `handler_value` handler).
    /// Using such a step as input to `route_on_confidence` is a pipeline authoring error.
    #[error("step '{0}' produced no confidence score — use a handler that emits confidence")]
    NoConfidence(String),
}
