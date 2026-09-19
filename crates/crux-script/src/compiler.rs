//! Typed pipeline compilation options and results.

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
