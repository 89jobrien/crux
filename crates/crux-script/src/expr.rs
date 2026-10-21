//! Parsing and evaluation of pipeline value references and string templates.

/// Minimal expression evaluator for `{{ path }}` references in YAML values.
///
/// Supports: `{{ input }}`, `{{ steps.<name>.output }}`, `{{ steps.<name>.confidence }}`.
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TemplateSegment {
    Text(String),
    Path(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParsedExpression {
    Literal(String),
    ExactPath(String),
    Interpolated(Vec<TemplateSegment>),
}

pub(crate) fn parse_expression(expr: &str) -> Result<ParsedExpression, ExprError> {
    let trimmed = expr.trim();
    if let Some(inner) = trimmed
        .strip_prefix("{{")
        .and_then(|value| value.strip_suffix("}}"))
    {
        let path = inner.trim();
        if path.is_empty() || path.contains("{{") || path.contains("}}") {
            return Err(ExprError::Syntax(expr.to_string()));
        }
        return Ok(ParsedExpression::ExactPath(path.to_string()));
    }

    if !expr.contains("{{") {
        return Ok(ParsedExpression::Literal(expr.to_string()));
    }

    let mut segments = Vec::new();
    let mut remaining = expr;
    while let Some(start) = remaining.find("{{") {
        if start > 0 {
            segments.push(TemplateSegment::Text(remaining[..start].to_string()));
        }
        let after_open = &remaining[start + 2..];
        let end = after_open
            .find("}}")
            .ok_or_else(|| ExprError::Syntax(expr.to_string()))?;
        let path = after_open[..end].trim();
        if path.is_empty() {
            return Err(ExprError::Syntax(expr.to_string()));
        }
        segments.push(TemplateSegment::Path(path.to_string()));
        remaining = &after_open[end + 2..];
    }
    if !remaining.is_empty() {
        segments.push(TemplateSegment::Text(remaining.to_string()));
    }
    Ok(ParsedExpression::Interpolated(segments))
}

/// Result of a completed step, used for expression resolution.
///
/// `confidence` is `None` for steps that produced no score (e.g. `handler_value` handlers).
/// Routing steps (`route_on_confidence`) that reference such a step will receive an
/// [`ExprError::NoConfidence`] rather than a spurious `1.0`.
///
/// `members` names the individual arms of a `join_all` group, so `{{ steps.<group>.<arm>.output }}`
/// can address one arm. Without it a group's output is an unnamed array and no arm is reachable.
#[derive(Debug, Clone)]
pub struct StepResult {
    pub output: Value,
    pub confidence: Option<f32>,
    /// Arm name -> arm output, populated only for `join_all` groups. Empty otherwise.
    pub members: HashMap<String, Value>,
}

impl StepResult {
    /// A plain step: no addressable members.
    pub fn scalar(output: Value, confidence: Option<f32>) -> Self {
        Self {
            output,
            confidence,
            members: HashMap::new(),
        }
    }
}

/// Per-iteration bindings exposed inside a loop construct's `steps:` block
/// (`for_each`, `while`, `repeat`, #84/#89) via `{{ iter.index }}` and
/// `{{ iter.<name> }}`.
#[derive(Debug, Clone, Default)]
pub struct IterFrame {
    pub index: usize,
    pub values: HashMap<String, Value>,
}

/// Evaluation context holding pipeline state.
#[derive(Clone)]
pub struct ExprContext {
    pub input: Value,
    pub steps: HashMap<String, StepResult>,
    /// Pipeline-level `vars:` bindings (#85), resolved once up front. Referenced
    /// from any step via `{{ vars.NAME }}` / `{{ vars.NAME.field }}`.
    pub vars: HashMap<String, Value>,
    /// Current loop iteration bindings, if evaluation is happening inside a loop
    /// construct's body (#84/#89). `None` outside any loop.
    pub iter: Option<IterFrame>,
}

impl ExprContext {
    /// Creates an evaluation context with input and no step, variable, or loop bindings.
    pub fn new(input: Value) -> Self {
        Self {
            input,
            steps: HashMap::new(),
            vars: HashMap::new(),
            iter: None,
        }
    }

    /// Resolve an expression string to a JSON value.
    ///
    /// - `{{ path }}` (entire string) — resolves to the typed JSON value at path.
    /// - Strings containing one or more `{{ path }}` snippets — interpolated: each
    ///   snippet is replaced with its string representation; result is a JSON string.
    /// - Plain strings with no `{{ }}` — returned unchanged as a JSON string.
    pub fn eval(&self, expr: &str) -> Result<Value, ExprError> {
        match parse_expression(expr)? {
            ParsedExpression::Literal(value) => Ok(Value::String(value)),
            ParsedExpression::ExactPath(path) => self.resolve_path(&path),
            ParsedExpression::Interpolated(segments) => {
                let mut result = String::with_capacity(expr.len());
                for segment in segments {
                    match segment {
                        TemplateSegment::Text(text) => result.push_str(&text),
                        TemplateSegment::Path(path) => match self.resolve_path(&path)? {
                            Value::String(value) => result.push_str(&value),
                            other => result.push_str(&other.to_string()),
                        },
                    }
                }
                Ok(Value::String(result))
            }
        }
    }

    /// Resolve an expression to f32 (for confidence routing).
    pub fn eval_f32(&self, expr: &str) -> Result<f32, ExprError> {
        let value = self.eval(expr)?;
        match value {
            Value::Number(n) => n.as_f64().map(|f| f as f32).ok_or(ExprError::NotNumeric),
            _ => Err(ExprError::NotNumeric),
        }
    }

    /// Resolve an expression to a bool (for `until:`, `condition:`, `break_if:`).
    pub fn eval_bool(&self, expr: &str) -> Result<bool, ExprError> {
        let value = self.eval(expr)?;
        match value {
            Value::Bool(b) => Ok(b),
            _ => Err(ExprError::NotBoolean),
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

        // `iter.index` — 0-based loop counter; `iter.<name>` / `iter.<name>.<sub>` —
        // the current item binding from a `for_each`/`while`/`repeat` loop (#84/#89).
        if let Some(rest) = path.strip_prefix("iter.") {
            let frame = self
                .iter
                .as_ref()
                .ok_or_else(|| ExprError::UnknownPath(path.to_string()))?;
            if rest == "index" {
                return Ok(Value::Number(serde_json::Number::from(frame.index)));
            }
            let mut parts = rest.splitn(2, '.');
            let name = parts.next().unwrap_or("");
            let value = frame
                .values
                .get(name)
                .ok_or_else(|| ExprError::UnknownPath(path.to_string()))?;
            return match parts.next() {
                Some(sub) => {
                    json_get(value, sub).ok_or_else(|| ExprError::UnknownPath(path.to_string()))
                }
                None => Ok(value.clone()),
            };
        }

        // `vars.<name>` — full value; `vars.<name>.<subfield>...` — dot-path into it.
        if let Some(rest) = path.strip_prefix("vars.") {
            let mut parts = rest.splitn(2, '.');
            let name = parts.next().unwrap_or("");
            let value = self
                .vars
                .get(name)
                .ok_or_else(|| ExprError::UnknownPath(path.to_string()))?;
            return match parts.next() {
                Some(sub) => {
                    json_get(value, sub).ok_or_else(|| ExprError::UnknownPath(path.to_string()))
                }
                None => Ok(value.clone()),
            };
        }

        // `steps.<group>.<arm>.<field>...` — one arm of a `join_all` group. Handled
        // before the 3-segment cases below because the arm name occupies a segment
        // that `steps.<name>.output` would otherwise consume as the trailing field.
        if let Some(rest) = path.strip_prefix("steps.") {
            let segments: Vec<&str> = rest.split('.').collect();
            if segments.len() >= 3 && segments[1] != "output" && segments[1] != "confidence" {
                let (group, arm) = (segments[0], segments[1]);
                let Some(result) = self.steps.get(group) else {
                    return Err(ExprError::UnknownStep(group.to_string()));
                };
                let Some(arm_output) = result.members.get(arm) else {
                    return Err(ExprError::UnknownArm {
                        group: group.to_string(),
                        arm: arm.to_string(),
                    });
                };
                let tail = segments[2..].join(".");
                let tail = tail.strip_prefix("output").unwrap_or(&tail);
                let tail = tail.strip_prefix('.').unwrap_or(tail);
                if tail.is_empty() {
                    return Ok(arm_output.clone());
                }
                return json_get(arm_output, tail)
                    .ok_or_else(|| ExprError::UnknownPath(path.to_string()));
            }
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
                let field_path = rest.strip_prefix("output.").unwrap_or(rest);
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
        owned = current.get(key)?.clone();
        current = &owned;
    }
    Some(current.clone())
}

#[derive(Debug, thiserror::Error)]
pub enum ExprError {
    #[error("syntax error in expression: {0}")]
    Syntax(String),
    #[error("unknown step: {0}")]
    UnknownStep(String),
    /// A `join_all` group exists but has no arm by that name. Carries both names so
    /// the diagnostic can suggest `steps.<group>.output` (the whole array).
    #[error("step group '{group}' has no arm '{arm}' — use steps.{group}.output for all arms")]
    UnknownArm { group: String, arm: String },
    #[error("unknown path: {0}")]
    UnknownPath(String),
    #[error("forward reference: {0}")]
    ForwardReference(String),
    #[error("invalid scope reference: {0}")]
    InvalidScope(String),
    #[error("value is not numeric")]
    NotNumeric,
    #[error("value is not a boolean")]
    NotBoolean,
    /// Step exists but produced no confidence score (e.g. a `handler_value` handler).
    /// Using such a step as input to `route_on_confidence` is a pipeline authoring error.
    #[error("step '{0}' produced no confidence score — use a handler that emits confidence")]
    NoConfidence(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Regression test for #103: a step output containing a field literally named
    /// "output" nested under the "output." namespace (i.e. `output.output.something`)
    /// must resolve via a single strip of the leading "output." prefix, not a repeated
    /// strip of every "output." occurrence.
    #[test]
    fn steps_output_dot_path_strips_prefix_once() {
        let mut ctx = ExprContext::new(Value::Null);
        ctx.steps.insert(
            "s1".to_string(),
            StepResult::scalar(json!({ "output": { "something": "value" } }), None),
        );

        let result = ctx
            .resolve_path("steps.s1.output.output.something")
            .expect("path should resolve");
        assert_eq!(result, json!("value"));
    }

    /// Regression test: join_all arm member access via `steps.<group>.<arm>.output`.
    /// Verifies that individual arms of a join_all group can be addressed by name.
    #[test]
    fn join_all_arm_member_access_by_name() {
        let mut ctx = ExprContext::new(Value::Null);
        let mut members = HashMap::new();
        members.insert("fetch_status".to_string(), json!({ "code": 200 }));
        members.insert("fetch_body".to_string(), json!("success"));

        ctx.steps.insert(
            "parallel_fetch".to_string(),
            StepResult {
                output: json!([{ "code": 200 }, "success"]),
                confidence: Some(0.95),
                members,
            },
        );

        // Access first arm
        let status = ctx
            .resolve_path("steps.parallel_fetch.fetch_status.output")
            .expect("fetch_status arm should resolve");
        assert_eq!(status, json!({ "code": 200 }));

        // Access second arm
        let body = ctx
            .resolve_path("steps.parallel_fetch.fetch_body.output")
            .expect("fetch_body arm should resolve");
        assert_eq!(body, json!("success"));

        // Unknown arm should error
        let err = ctx.resolve_path("steps.parallel_fetch.unknown_arm.output");
        assert!(matches!(err, Err(ExprError::UnknownArm { .. })));
    }
}
