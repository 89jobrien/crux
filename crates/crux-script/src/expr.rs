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

    /// Resolve an expression string to a JSON value.
    ///
    /// - `{{ path }}` (entire string) — resolves to the typed JSON value at path.
    /// - Strings containing one or more `{{ path }}` snippets — interpolated: each
    ///   snippet is replaced with its string representation; result is a JSON string.
    /// - Plain strings with no `{{ }}` — returned unchanged as a JSON string.
    pub fn eval(&self, expr: &str) -> Result<Value, ExprError> {
        let trimmed = expr.trim();

        // Fast path: entire string is a single template — return typed value.
        if let Some(inner) = trimmed
            .strip_prefix("{{")
            .and_then(|s| s.strip_suffix("}}"))
        {
            return self.resolve_path(inner.trim());
        }

        // No template markers at all — return as-is.
        if !expr.contains("{{") {
            return Ok(Value::String(expr.to_string()));
        }

        // Interpolation: replace every `{{ path }}` occurrence within the string.
        let mut result = String::with_capacity(expr.len());
        let mut remaining = expr;
        while let Some(start) = remaining.find("{{") {
            result.push_str(&remaining[..start]);
            let after_open = &remaining[start + 2..];
            let end = after_open
                .find("}}")
                .ok_or_else(|| ExprError::Syntax(expr.to_string()))?;
            let path = after_open[..end].trim();
            let value = self.resolve_path(path)?;
            match value {
                Value::String(s) => result.push_str(&s),
                other => result.push_str(&other.to_string()),
            }
            remaining = &after_open[end + 2..];
        }
        result.push_str(remaining);
        Ok(Value::String(result))
    }

    /// Resolve an expression to f32 (for confidence routing).
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

        const MAX_PATH_SEGMENTS: usize = 3;
        let parts: Vec<&str> = path.splitn(MAX_PATH_SEGMENTS, '.').collect();
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
                // TODO(review): trim_start_matches strips repeated "output." prefixes;
                //   use strip_prefix("output.").unwrap_or(rest) for single-strip
                let field_path = rest.trim_start_matches("output.");
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
