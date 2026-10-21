//! Compatibility validation views over typed pipeline compilation.

use std::fmt;

use miette::Diagnostic;

use crate::compiler::{CompileOptions, compile_cruxfile, compile_pipeline};
use crate::registry::HandlerRegistry;
use crate::schema::{CruxfileDef, PipelineDef, StepDef};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

impl DiagnosticSeverity {
    fn to_miette_severity(self) -> miette::Severity {
        match self {
            Self::Error => miette::Severity::Error,
            Self::Warning => miette::Severity::Warning,
        }
    }
}

impl fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error => f.write_str("error"),
            Self::Warning => f.write_str("warning"),
        }
    }
}

/// Stable category attached to a pipeline compilation diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidationCode {
    DuplicateName,
    UnknownHandler,
    UnknownAgent,
    MissingContract,
    MissingInputSchema,
    InvalidArguments,
    InvalidExpression,
    UnknownReference,
    ForwardReference,
    InvalidScope,
    TypeMismatch,
    DynamicBoundary,
    InvalidControlFlow,
    UnreachableStep,
    InvalidRoute,
    InvalidBudget,
    TargetResolution,
    LegacyValidation,
}

impl fmt::Display for ValidationCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            Self::DuplicateName => "duplicate_name",
            Self::UnknownHandler => "unknown_handler",
            Self::UnknownAgent => "unknown_agent",
            Self::MissingContract => "missing_contract",
            Self::MissingInputSchema => "missing_input_schema",
            Self::InvalidArguments => "invalid_arguments",
            Self::InvalidExpression => "invalid_expression",
            Self::UnknownReference => "unknown_reference",
            Self::ForwardReference => "forward_reference",
            Self::InvalidScope => "invalid_scope",
            Self::TypeMismatch => "type_mismatch",
            Self::DynamicBoundary => "dynamic_boundary",
            Self::InvalidControlFlow => "invalid_control_flow",
            Self::UnreachableStep => "unreachable_step",
            Self::InvalidRoute => "invalid_route",
            Self::InvalidBudget => "invalid_budget",
            Self::TargetResolution => "target_resolution",
            Self::LegacyValidation => "legacy_validation",
        };
        f.write_str(code)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationDiagnostic {
    pub code: ValidationCode,
    pub severity: DiagnosticSeverity,
    pub location: String,
    pub message: String,
}

impl ValidationDiagnostic {
    /// Creates an error diagnostic with the legacy validation code.
    pub fn error(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self::error_with_code(ValidationCode::LegacyValidation, location, message)
    }

    /// Creates a warning diagnostic with the legacy validation code.
    pub fn warning(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self::warning_with_code(ValidationCode::LegacyValidation, location, message)
    }

    /// Creates an error diagnostic with an explicit validation code.
    pub fn error_with_code(
        code: ValidationCode,
        location: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity: DiagnosticSeverity::Error,
            location: location.into(),
            message: message.into(),
        }
    }

    /// Creates a warning diagnostic with an explicit validation code.
    pub fn warning_with_code(
        code: ValidationCode,
        location: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
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
        Some(Box::new(format!("crux::validate::{}", self.code)))
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
    /// Reports whether validation produced no error diagnostics.
    pub fn is_ok(&self) -> bool {
        self.error_count() == 0
    }

    /// Counts diagnostics classified as errors.
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            .count()
    }

    /// Counts diagnostics classified as warnings.
    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
            .count()
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
                self.diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic as &dyn Diagnostic),
            ))
        }
    }
}

/// Validate a parsed pipeline through the canonical permissive compiler.
pub fn validate_pipeline(pipeline: &PipelineDef, registry: &HandlerRegistry) -> ValidationReport {
    let mut diagnostics = compile_pipeline(pipeline, registry, CompileOptions::permissive())
        .diagnostics()
        .to_vec();
    retain_legacy_argument_compatibility(pipeline, registry, &mut diagnostics);
    ValidationReport { diagnostics }
}

/// Validate a Cruxfile through the canonical permissive compiler.
pub fn validate_cruxfile(cruxfile: &CruxfileDef, registry: &HandlerRegistry) -> ValidationReport {
    ValidationReport {
        diagnostics: compile_cruxfile(cruxfile, registry, CompileOptions::permissive())
            .diagnostics()
            .to_vec(),
    }
}

fn retain_legacy_argument_compatibility(
    pipeline: &PipelineDef,
    registry: &HandlerRegistry,
    diagnostics: &mut Vec<ValidationDiagnostic>,
) {
    for (index, step) in pipeline.steps.iter().enumerate() {
        let StepDef::Step(node) = step else {
            continue;
        };
        let handler = node.handler.as_deref().unwrap_or(&node.step);
        let Some(metadata) = registry.get_metadata(handler) else {
            continue;
        };
        let missing = metadata
            .args
            .args
            .iter()
            .filter(|argument| {
                argument.required
                    && node
                        .args
                        .as_ref()
                        .and_then(serde_json::Value::as_object)
                        .is_none_or(|args| !args.contains_key(&argument.name))
            })
            .map(|argument| argument.name.as_str())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            diagnostics.insert(
                0,
                ValidationDiagnostic::error_with_code(
                    ValidationCode::InvalidArguments,
                    format!("steps[{index}]"),
                    format!(
                        "handler '{handler}' is missing required args: {}",
                        missing.join(", ")
                    ),
                ),
            );
        }
    }
}
