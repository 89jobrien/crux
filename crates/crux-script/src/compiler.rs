//! Typed pipeline compilation options and results.

use crate::ir::{TypedPipeline, TypedStep, TypedValue};
use crate::registry::HandlerRegistry;
use crate::schema::{PipelineDef, StepDef};
use crate::validator::{DiagnosticSeverity, ValidationCode, ValidationDiagnostic};

/// Static validation strictness used while compiling pipeline definitions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompileMode {
    /// Preserve dynamic extension points as warnings where execution remains safe.
    #[default]
    Permissive,
    /// Require complete contracts and reject dynamic boundaries.
    Strict,
}

/// Options controlling typed pipeline compilation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompileOptions {
    mode: CompileMode,
}

impl CompileOptions {
    /// Create permissive compilation options.
    pub const fn permissive() -> Self {
        Self {
            mode: CompileMode::Permissive,
        }
    }

    /// Create strict compilation options.
    pub const fn strict() -> Self {
        Self {
            mode: CompileMode::Strict,
        }
    }

    /// Return the selected compile mode.
    pub const fn mode(self) -> CompileMode {
        self.mode
    }

    /// Return the severity assigned to one diagnostic code in this mode.
    pub const fn severity_for(self, code: ValidationCode) -> DiagnosticSeverity {
        match (self.mode, code) {
            (
                CompileMode::Permissive,
                ValidationCode::UnknownHandler
                | ValidationCode::UnknownAgent
                | ValidationCode::MissingContract
                | ValidationCode::MissingInputSchema
                | ValidationCode::DynamicBoundary,
            ) => DiagnosticSeverity::Warning,
            _ => DiagnosticSeverity::Error,
        }
    }
}

/// Compiler output containing diagnostics and an optional executable artifact.
#[derive(Debug, Clone)]
pub struct Compilation<T> {
    artifact: Option<T>,
    diagnostics: Vec<ValidationDiagnostic>,
}

impl<T> Compilation<T> {
    /// Build a compilation result, discarding the artifact when any error exists.
    pub fn new(artifact: Option<T>, diagnostics: Vec<ValidationDiagnostic>) -> Self {
        let has_errors = diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error);
        Self {
            artifact: if has_errors { None } else { artifact },
            diagnostics,
        }
    }

    /// Return the compiled artifact when this result is executable.
    pub fn artifact(&self) -> Option<&T> {
        self.artifact.as_ref()
    }

    /// Return all compiler diagnostics.
    pub fn diagnostics(&self) -> &[ValidationDiagnostic] {
        &self.diagnostics
    }

    /// Count error diagnostics.
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            .count()
    }

    /// Count warning diagnostics.
    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
            .count()
    }

    /// Return whether compilation produced no errors.
    pub fn is_ok(&self) -> bool {
        self.error_count() == 0
    }

    /// Return whether an artifact is available for execution.
    pub fn is_executable(&self) -> bool {
        self.is_ok() && self.artifact.is_some()
    }

    /// Consume the result and return its artifact.
    pub fn into_artifact(self) -> Option<T> {
        self.artifact
    }

    /// Consume the result into its artifact and diagnostics.
    pub fn into_parts(self) -> (Option<T>, Vec<ValidationDiagnostic>) {
        (self.artifact, self.diagnostics)
    }
}

/// Compile simple handler steps into resolved runtime-only typed IR.
pub fn compile_pipeline(
    definition: &PipelineDef,
    registry: &HandlerRegistry,
    options: CompileOptions,
) -> Compilation<TypedPipeline> {
    let mut diagnostics = Vec::new();
    let mut steps = Vec::with_capacity(definition.steps.len());
    let mut unresolved = false;

    for (index, step) in definition.steps.iter().enumerate() {
        let location = format!("steps[{index}]");
        let StepDef::Step(node) = step else {
            diagnostics.push(ValidationDiagnostic::error_with_code(
                ValidationCode::InvalidControlFlow,
                location,
                "typed compilation for this combinator is not implemented",
            ));
            unresolved = true;
            continue;
        };

        let handler_name = node.handler.as_deref().unwrap_or(&node.step);
        let Some(runner) = registry.runner(handler_name) else {
            diagnostics.push(diagnostic_for_mode(
                options,
                ValidationCode::UnknownHandler,
                &location,
                format!("handler '{handler_name}' is not registered"),
            ));
            unresolved = true;
            continue;
        };

        if !runner.metadata().has_complete_contract() {
            diagnostics.push(diagnostic_for_mode(
                options,
                ValidationCode::MissingContract,
                &location,
                format!("handler '{handler_name}' has no complete contract"),
            ));
            if options.mode() == CompileMode::Strict {
                unresolved = true;
                continue;
            }
        }

        let args = match node.args.as_ref().map(TypedValue::compile).transpose() {
            Ok(args) => args,
            Err(error) => {
                diagnostics.push(ValidationDiagnostic::error_with_code(
                    ValidationCode::InvalidExpression,
                    format!("{location}.args"),
                    error.to_string(),
                ));
                unresolved = true;
                continue;
            }
        };

        steps.push(TypedStep {
            node: node.clone(),
            runner,
            args,
        });
    }

    let artifact = (!unresolved).then(|| TypedPipeline::new(definition, steps));
    Compilation::new(artifact, diagnostics)
}

fn diagnostic_for_mode(
    options: CompileOptions,
    code: ValidationCode,
    location: impl Into<String>,
    message: impl Into<String>,
) -> ValidationDiagnostic {
    match options.severity_for(code) {
        DiagnosticSeverity::Error => ValidationDiagnostic::error_with_code(code, location, message),
        DiagnosticSeverity::Warning => {
            ValidationDiagnostic::warning_with_code(code, location, message)
        }
    }
}
