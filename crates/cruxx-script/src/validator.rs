//! Static pipeline validation against registered handler metadata.

use std::fmt;

use serde_json::Value;

use crate::metadata::ArgType;
use crate::registry::HandlerRegistry;
use crate::schema::{ArmDef, PipelineDef, RouteBranch, StepDef};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

impl fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiagnosticSeverity::Error => f.write_str("error"),
            DiagnosticSeverity::Warning => f.write_str("warning"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationDiagnostic {
    pub severity: DiagnosticSeverity,
    pub location: String,
    pub message: String,
}

impl ValidationDiagnostic {
    pub fn error(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            location: location.into(),
            message: message.into(),
        }
    }

    pub fn warning(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: DiagnosticSeverity::Warning,
            location: location.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationReport {
    pub diagnostics: Vec<ValidationDiagnostic>,
}

impl ValidationReport {
    pub fn is_ok(&self) -> bool {
        self.error_count() == 0
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .count()
    }

    fn push(&mut self, diagnostic: ValidationDiagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

/// Validate a parsed pipeline against the registered handler metadata.
pub fn validate_pipeline(pipeline: &PipelineDef, registry: &HandlerRegistry) -> ValidationReport {
    let mut report = ValidationReport::default();

    for (idx, step) in pipeline.steps.iter().enumerate() {
        let location = format!("steps[{idx}]");
        match step {
            StepDef::Step(node) => {
                let handler = node.handler.as_deref().unwrap_or(&node.step);
                validate_handler_ref(
                    &mut report,
                    registry,
                    &location,
                    handler,
                    node.args.as_ref(),
                );
            }
            StepDef::Delegate(node) => {
                if registry.get_agent(&node.delegate).is_none() {
                    report.push(ValidationDiagnostic::warning(
                        &location,
                        format!("agent '{}' is not registered", node.delegate),
                    ));
                }
            }
            StepDef::Pipe(node) => {
                for (stage_idx, arm) in node.stages.iter().enumerate() {
                    validate_arm(
                        &mut report,
                        registry,
                        &format!("{location}.stages[{stage_idx}]"),
                        arm,
                    );
                }
            }
            StepDef::JoinAll(node) => {
                for (arm_idx, arm) in node.arms.iter().enumerate() {
                    validate_arm(
                        &mut report,
                        registry,
                        &format!("{location}.arms[{arm_idx}]"),
                        arm,
                    );
                }
            }
            StepDef::RouteOnConfidence(node) => {
                validate_routes(&mut report, &location, &node.routes);
                for (route_idx, branch) in node.routes.iter().enumerate() {
                    validate_handler_ref(
                        &mut report,
                        registry,
                        &format!("{location}.routes[{route_idx}]"),
                        &branch.handler,
                        branch.args.as_ref(),
                    );
                }
            }
            StepDef::Speculate(node) => {
                for (arm_idx, arm) in node.arms.iter().enumerate() {
                    validate_arm(
                        &mut report,
                        registry,
                        &format!("{location}.arms[{arm_idx}]"),
                        arm,
                    );
                }
            }
        }
    }

    report
}

fn validate_arm(
    report: &mut ValidationReport,
    registry: &HandlerRegistry,
    location: &str,
    arm: &ArmDef,
) {
    validate_handler_ref(report, registry, location, arm.handler_name(), arm.args());
}

fn validate_handler_ref(
    report: &mut ValidationReport,
    registry: &HandlerRegistry,
    location: &str,
    handler: &str,
    args: Option<&Value>,
) {
    let Some(metadata) = registry.get_metadata(handler) else {
        if registry.get_handler(handler).is_none() {
            report.push(ValidationDiagnostic::error(
                location,
                format!("handler '{handler}' is not registered"),
            ));
        } else {
            report.push(ValidationDiagnostic::warning(
                location,
                format!("handler '{handler}' has no metadata — args not validated"),
            ));
        }
        return;
    };

    let Some(schema_args) = args else {
        if metadata.args.has_required_args() {
            let missing = metadata
                .args
                .args
                .iter()
                .filter(|spec| spec.required)
                .map(|spec| spec.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            report.push(ValidationDiagnostic::error(
                location,
                format!("handler '{handler}' is missing required args: {missing}"),
            ));
        }
        return;
    };

    let Some(arg_map) = schema_args.as_object() else {
        report.push(ValidationDiagnostic::error(
            location,
            format!("handler '{handler}' args must be an object"),
        ));
        return;
    };

    for spec in &metadata.args.args {
        let Some(value) = arg_map.get(&spec.name) else {
            if spec.required {
                report.push(ValidationDiagnostic::error(
                    location,
                    format!(
                        "handler '{handler}' is missing required arg '{}'",
                        spec.name
                    ),
                ));
            }
            continue;
        };

        if is_template_string(value) {
            continue;
        }

        if !spec.arg_type.matches(value) {
            report.push(ValidationDiagnostic::error(
                location,
                format!(
                    "handler '{handler}' arg '{}' expected {}, got {}",
                    spec.name,
                    display_arg_type(spec.arg_type),
                    display_value_type(value)
                ),
            ));
        }
    }

    if !metadata.args.allow_extra {
        for key in arg_map.keys() {
            if metadata.args.get(key).is_none() {
                report.push(ValidationDiagnostic::error(
                    location,
                    format!("handler '{handler}' received unexpected arg '{key}'"),
                ));
            }
        }
    }
}

fn is_template_string(value: &Value) -> bool {
    value
        .as_str()
        .map(|s| s.trim_start().starts_with("{{"))
        .unwrap_or(false)
}

fn display_arg_type(arg_type: ArgType) -> &'static str {
    match arg_type {
        ArgType::Any => "any",
        ArgType::String => "string",
        ArgType::Number => "number",
        ArgType::Integer => "integer",
        ArgType::Boolean => "boolean",
        ArgType::Object => "object",
        ArgType::Array => "array",
    }
}

fn display_value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[derive(Debug, Clone, Copy)]
struct ParsedRange {
    lo: f32,
    hi: f32,
    include_hi: bool,
}

fn validate_routes(report: &mut ValidationReport, location: &str, routes: &[RouteBranch]) {
    let mut parsed = Vec::new();

    for (idx, branch) in routes.iter().enumerate() {
        match parse_range(&branch.range) {
            Ok(range) => {
                if range.lo < 0.0 || range.hi > 1.0 {
                    report.push(ValidationDiagnostic::error(
                        format!("{location}.routes[{idx}]"),
                        format!(
                            "confidence range '{}' must stay within [0.0, 1.0]",
                            branch.range
                        ),
                    ));
                }
                if range.lo > range.hi || (range.lo == range.hi && !range.include_hi) {
                    report.push(ValidationDiagnostic::error(
                        format!("{location}.routes[{idx}]"),
                        format!("confidence range '{}' is empty", branch.range),
                    ));
                }
                parsed.push((idx, range));
            }
            Err(e) => report.push(ValidationDiagnostic::error(
                format!("{location}.routes[{idx}]"),
                format!("invalid confidence range '{}': {e}", branch.range),
            )),
        }
    }

    parsed.sort_by(|a, b| {
        a.1.lo
            .partial_cmp(&b.1.lo)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for pair in parsed.windows(2) {
        let (left_idx, left) = pair[0];
        let (right_idx, right) = pair[1];
        if ranges_overlap(left, right) {
            report.push(ValidationDiagnostic::error(
                location,
                format!("confidence ranges for routes {left_idx} and {right_idx} overlap"),
            ));
        }
    }
}

fn parse_range(s: &str) -> Result<ParsedRange, &'static str> {
    let s = s.trim();
    if !(s.starts_with('[') || s.starts_with('(')) {
        return Err("missing opening bracket");
    }
    let include_hi = if s.ends_with(']') {
        true
    } else if s.ends_with(')') {
        false
    } else {
        return Err("missing closing bracket");
    };

    let inner = &s[1..s.len() - 1];
    let Some((lo, hi)) = inner.split_once(',') else {
        return Err("expected lower and upper bounds");
    };
    let lo = lo
        .trim()
        .parse::<f32>()
        .map_err(|_| "invalid lower bound")?;
    let hi = hi
        .trim()
        .parse::<f32>()
        .map_err(|_| "invalid upper bound")?;
    Ok(ParsedRange { lo, hi, include_hi })
}

fn ranges_overlap(left: ParsedRange, right: ParsedRange) -> bool {
    if left.hi > right.lo {
        return true;
    }
    left.hi == right.lo && left.include_hi
}
