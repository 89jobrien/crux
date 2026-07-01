//! Static pipeline validation against registered handler metadata.

use std::fmt;

use miette::Diagnostic;
use serde_json::Value;

use crate::metadata::ArgType;
use crate::registry::HandlerRegistry;
use crate::resolve::TargetResolver;
use crate::schema::{ArmDef, CruxfileDef, PipelineDef, RouteBranch, StepDef};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

impl DiagnosticSeverity {
    fn to_miette_severity(self) -> miette::Severity {
        match self {
            DiagnosticSeverity::Error => miette::Severity::Error,
            DiagnosticSeverity::Warning => miette::Severity::Warning,
        }
    }
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

impl fmt::Display for ValidationDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

impl std::error::Error for ValidationDiagnostic {}

impl Diagnostic for ValidationDiagnostic {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(format!("crux::validate::{}", self.severity)))
    }

    fn severity(&self) -> Option<miette::Severity> {
        Some(self.severity.to_miette_severity())
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        None
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

impl fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "pipeline validation: {} error(s), {} warning(s)",
            self.error_count(),
            self.warning_count()
        )
    }
}

impl std::error::Error for ValidationReport {}

impl Diagnostic for ValidationReport {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new("crux::validate"))
    }

    fn severity(&self) -> Option<miette::Severity> {
        if self.error_count() > 0 {
            Some(miette::Severity::Error)
        } else if self.warning_count() > 0 {
            Some(miette::Severity::Warning)
        } else {
            Some(miette::Severity::Advice)
        }
    }

    fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
        if self.diagnostics.is_empty() {
            None
        } else {
            Some(Box::new(
                self.diagnostics.iter().map(|d| d as &dyn Diagnostic),
            ))
        }
    }
}

/// Validate a parsed pipeline against the registered handler metadata.
pub fn validate_pipeline(pipeline: &PipelineDef, registry: &HandlerRegistry) -> ValidationReport {
    let mut report = ValidationReport::default();
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (idx, step) in pipeline.steps.iter().enumerate() {
        let location = format!("steps[{idx}]");

        let step_name = match step {
            StepDef::Step(n) => n.step.as_str(),
            StepDef::Delegate(n) => n.name.as_deref().unwrap_or(&n.delegate),
            StepDef::Pipe(n) => n.pipe.as_str(),
            StepDef::JoinAll(n) => n.join_all.as_str(),
            StepDef::RouteOnConfidence(n) => n.route_on_confidence.as_str(),
            StepDef::Speculate(n) => n.speculate.as_str(),
        };
        if !seen_names.insert(step_name.to_string()) {
            report.push(ValidationDiagnostic::error(
                &location,
                format!("duplicate step name '{step_name}'"),
            ));
        }

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

/// Validate a Cruxfile: each target's steps, dependency references, cycles, and default target.
pub fn validate_cruxfile(cruxfile: &CruxfileDef, registry: &HandlerRegistry) -> ValidationReport {
    let mut report = ValidationReport::default();

    // Check default target exists.
    if !cruxfile.targets.contains_key(&cruxfile.default) {
        report.push(ValidationDiagnostic::error(
            "default",
            format!(
                "default target '{}' is not defined in targets",
                cruxfile.default
            ),
        ));
    }

    // Check dependency graph (unknown deps + cycles).
    if let Err(e) = TargetResolver::new(cruxfile) {
        report.push(ValidationDiagnostic::error("targets", e.to_string()));
    }

    // Validate each target's steps as if it were a pipeline.
    for (name, target) in &cruxfile.targets {
        let pipeline = PipelineDef {
            pipeline: name.clone(),
            budget: target.budget.clone().or_else(|| cruxfile.budget.clone()),
            steps: target.steps.clone(),
        };
        let target_report = validate_pipeline(&pipeline, registry);
        for mut diag in target_report.diagnostics {
            diag.location = format!("targets.{name}.{}", diag.location);
            report.push(diag);
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
        if registry.get_handler(handler).is_some() {
            report.push(ValidationDiagnostic::warning(
                location,
                format!("handler '{handler}' has no metadata — args not validated"),
            ));
        } else {
            // Distinguish known namespace (error) from unknown namespace (warning).
            let known_ns = handler
                .split_once("::")
                .map(|(ns, _)| registry.registered_namespaces().contains(ns))
                .unwrap_or(false);
            if known_ns {
                report.push(ValidationDiagnostic::error(
                    location,
                    format!("handler '{handler}' is not registered (namespace exists)"),
                ));
            } else {
                report.push(ValidationDiagnostic::warning(
                    location,
                    format!("handler '{handler}' is not registered"),
                ));
            }
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
