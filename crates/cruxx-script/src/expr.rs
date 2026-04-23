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

        // `input.<field>.<subfield>...` — dot-path into the pipeline input
        if let Some(rest) = path.strip_prefix("input.") {
            return json_get(&self.input, rest)
                .ok_or_else(|| ExprError::UnknownPath(path.to_string()));
        }

        let parts: Vec<&str> = path.splitn(3, '.').collect();
        match parts.as_slice() {
            // `steps.<name>.output` — full output value
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
                    // safe: confidence is always finite (NaN rejected in HandlerOutput::with_confidence)
                    serde_json::Number::from_f64(score as f64)
                        .expect("confidence is always finite"),
                ))
            }
            // `steps.<name>.output.<field>...` — dot-path into a step's output
            ["steps", name, rest] if rest.starts_with("output.") => {
                let step = self
                    .steps
                    .get(*name)
                    .ok_or_else(|| ExprError::UnknownStep((*name).to_string()))?;
                let field_path = rest.strip_prefix("output.").unwrap();
                json_get(&step.output, field_path)
                    .ok_or_else(|| ExprError::UnknownPath(path.to_string()))
            }
            _ => Err(ExprError::UnknownPath(path.to_string())),
        }
    }
}

/// Walk a dot-separated key path into a JSON value, returning the nested value if found.
fn json_get(value: &Value, path: &str) -> Option<Value> {
    let mut current = value;
    let mut owned;
    for key in path.split('.') {
        match current.get(key) {
            Some(v) => {
                owned = v.clone();
                current = &owned;
            }
            None => return None,
        }
    }
    Some(current.clone())
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
